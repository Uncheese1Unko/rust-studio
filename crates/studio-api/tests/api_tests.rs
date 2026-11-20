use studio_api::{handle_request, ApiState, Request};
use studio_core::Project;

fn state() -> ApiState {
    ApiState::new(Project::new("api test"))
}

fn req(method: &str, path: &str, body: &str) -> Request {
    Request {
        method: method.to_string(),
        path: path.to_string(),
        body: body.as_bytes().to_vec(),
    }
}

fn get_dump(state: &ApiState) -> String {
    let res = handle_request(state, &req("GET", "/project", ""));
    assert_eq!(res.status, 200);
    res.body
}

#[test]
fn get_project_returns_state_dump() {
    let state = state();
    let body = get_dump(&state);
    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(value["name"], "api test");
    assert_eq!(value["tracks"], serde_json::json!([]));
}

#[test]
fn post_command_applies_and_dump_reflects_it() {
    let state = state();

    let res = handle_request(
        &state,
        &req(
            "POST",
            "/commands",
            r#"{"type":"add_track","name":"V1","kind":"video"}"#,
        ),
    );
    assert_eq!(res.status, 200);
    let value: serde_json::Value = serde_json::from_str(&res.body).unwrap();
    assert_eq!(value["ok"], true);
    let track_id = value["created_id"].as_u64().unwrap();

    let res = handle_request(
        &state,
        &req(
            "POST",
            "/commands",
            &format!(
                r#"{{"type":"add_clip","track_id":{track_id},"name":"op","start_ms":0,"duration_ms":1500,"source":"media/op.mp4"}}"#
            ),
        ),
    );
    assert_eq!(res.status, 200);

    let dump = get_dump(&state);
    let value: serde_json::Value = serde_json::from_str(&dump).unwrap();
    assert_eq!(value["tracks"][0]["name"], "V1");
    assert_eq!(value["tracks"][0]["clips"][0]["name"], "op");
    assert_eq!(value["tracks"][0]["clips"][0]["duration_ms"], 1500);
}

#[test]
fn failing_command_returns_error_and_leaves_state_unchanged() {
    let state = state();
    let before = get_dump(&state);

    // 存在しないトラックへのクリップ追加はコマンド層で失敗する
    let res = handle_request(
        &state,
        &req(
            "POST",
            "/commands",
            r#"{"type":"add_clip","track_id":999,"name":"x","start_ms":0,"duration_ms":100,"source":"s"}"#,
        ),
    );
    assert_eq!(res.status, 400);
    let value: serde_json::Value = serde_json::from_str(&res.body).unwrap();
    assert_eq!(value["ok"], false);
    assert!(value["error"].as_str().unwrap().contains("999"));

    assert_eq!(get_dump(&state), before);
}

#[test]
fn malformed_json_returns_400_and_leaves_state_unchanged() {
    let state = state();
    let before = get_dump(&state);

    let res = handle_request(&state, &req("POST", "/commands", "{ not json"));
    assert_eq!(res.status, 400);
    let value: serde_json::Value = serde_json::from_str(&res.body).unwrap();
    assert_eq!(value["ok"], false);

    assert_eq!(get_dump(&state), before);
}

#[test]
fn unknown_path_is_404_and_wrong_method_is_405() {
    let state = state();
    assert_eq!(handle_request(&state, &req("GET", "/nope", "")).status, 404);
    assert_eq!(
        handle_request(&state, &req("DELETE", "/project", "")).status,
        405
    );
    assert_eq!(
        handle_request(&state, &req("GET", "/commands", "")).status,
        405
    );
}

#[test]
fn server_round_trip_over_a_real_socket() {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let state = ApiState::new(Project::new("socket test"));
    std::thread::spawn(move || studio_api::serve(listener, state));

    // POST でコマンドを送る
    let body = r#"{"type":"add_track","name":"A1","kind":"audio"}"#;
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "POST /commands HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .unwrap();
    let mut text = String::new();
    stream.read_to_string(&mut text).unwrap();
    assert!(text.starts_with("HTTP/1.1 200"), "response: {text}");
    assert!(text.contains("\"ok\":true"), "response: {text}");

    // GET でダンプに反映されていることを見る
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    write!(
        stream,
        "GET /project HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n"
    )
    .unwrap();
    let mut text = String::new();
    stream.read_to_string(&mut text).unwrap();
    assert!(text.starts_with("HTTP/1.1 200"), "response: {text}");
    assert!(text.contains("\"A1\""), "response: {text}");
}
