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
fn server_survives_abruptly_closed_connections() {
    use std::io::Write;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let state = ApiState::new(Project::new("robustness test"));
    std::thread::spawn(move || studio_api::serve(listener, state));

    // 応答を読まずに切る接続を繰り返す（レビューで再現された
    // 「乱暴な切断でサーバーが死ぬ」不具合の回帰テスト）
    for _ in 0..10 {
        let body = r#"{"type":"add_track","name":"T","kind":"audio"}"#;
        if let Ok(mut stream) = std::net::TcpStream::connect(addr) {
            let _ = write!(
                stream,
                "POST /commands HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            // 応答を読まずにすぐ閉じる
            drop(stream);
        }
        // 何も送らずに閉じる接続も混ぜる
        if let Ok(stream) = std::net::TcpStream::connect(addr) {
            drop(stream);
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(300));

    // サーバーがまだ生きていて、通常のリクエストに応えること
    let mut stream = std::net::TcpStream::connect(addr)
        .expect("server must still accept connections after abrupt disconnects");
    write!(
        stream,
        "GET /project HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n"
    )
    .unwrap();
    use std::io::Read;
    let mut text = String::new();
    stream.read_to_string(&mut text).unwrap();
    assert!(text.starts_with("HTTP/1.1 200"), "response: {text}");
}

#[test]
fn server_does_not_wedge_on_an_idle_connection() {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let state = ApiState::new(Project::new("idle test"));
    std::thread::spawn(move || studio_api::serve(listener, state));

    // 何も送らない接続を保持し続ける（レビューで再現された
    // 「アイドル接続でサーバーが固まる」不具合の回帰テスト）
    let _idle = std::net::TcpStream::connect(addr).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));

    // 後続のリクエストが、サーバー側のリクエスト期限内に処理されること
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(25)))
        .unwrap();
    write!(
        stream,
        "GET /project HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n"
    )
    .unwrap();
    let mut text = String::new();
    stream
        .read_to_string(&mut text)
        .expect("server must answer once the idle connection hits its deadline");
    assert!(text.starts_with("HTTP/1.1 200"), "response: {text}");
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

#[test]
fn rename_track_command_applies_via_api() {
    // Issue #55: HTTP API は Command を汎用的にデシリアライズするため、
    // rename_track も追加対応なしで実行できることを確認する。
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
    let track_id = value["created_id"].as_u64().unwrap();

    let res = handle_request(
        &state,
        &req(
            "POST",
            "/commands",
            &format!(r#"{{"type":"rename_track","track_id":{track_id},"name":"メイン映像"}}"#),
        ),
    );
    assert_eq!(res.status, 200, "{}", res.body);
    let dump = get_dump(&state);
    let value: serde_json::Value = serde_json::from_str(&dump).unwrap();
    assert_eq!(value["tracks"][0]["name"], "メイン映像");

    // 空白名は 400 で、状態は変わらない
    let res = handle_request(
        &state,
        &req(
            "POST",
            "/commands",
            &format!(r#"{{"type":"rename_track","track_id":{track_id},"name":" "}}"#),
        ),
    );
    assert_eq!(res.status, 400);
    assert_eq!(get_dump(&state), dump);
}

#[test]
fn set_track_color_command_applies_via_api() {
    // Issue #58: HTTP API も Command の汎用デシリアライズで
    // set_track_color を追加対応なしに実行できることを確認する。
    let state = state();
    let res = handle_request(
        &state,
        &req(
            "POST",
            "/commands",
            r#"{"type":"add_track","name":"A1","kind":"audio"}"#,
        ),
    );
    assert_eq!(res.status, 200);
    let value: serde_json::Value = serde_json::from_str(&res.body).unwrap();
    let track_id = value["created_id"].as_u64().unwrap();

    let res = handle_request(
        &state,
        &req(
            "POST",
            "/commands",
            &format!(r#"{{"type":"set_track_color","track_id":{track_id},"color":"sky"}}"#),
        ),
    );
    assert_eq!(res.status, 200, "{}", res.body);
    let dump = get_dump(&state);
    let project: studio_core::Project = serde_json::from_str(&dump).unwrap();
    assert_eq!(project.tracks[0].color, Some(studio_core::ColorTag::Sky));
}

#[test]
fn undo_redo_endpoints_restore_state() {
    // Issue #29: 適用→undo→redo が全量ダンプで観測できる。
    let state = state();
    let initial = get_dump(&state);
    let res = handle_request(
        &state,
        &req(
            "POST",
            "/commands",
            r#"{"type":"add_track","name":"A1","kind":"audio"}"#,
        ),
    );
    assert_eq!(res.status, 200);
    let applied = get_dump(&state);
    assert_ne!(applied, initial);

    let res = handle_request(&state, &req("POST", "/undo", ""));
    assert_eq!(res.status, 200);
    let value: serde_json::Value = serde_json::from_str(&res.body).unwrap();
    assert_eq!(value["undone"], true);
    assert_eq!(get_dump(&state), initial);

    let res = handle_request(&state, &req("POST", "/redo", ""));
    assert_eq!(res.status, 200);
    let value: serde_json::Value = serde_json::from_str(&res.body).unwrap();
    assert_eq!(value["redone"], true);
    assert_eq!(get_dump(&state), applied);
}

#[test]
fn undo_on_empty_history_is_a_noop_not_an_error() {
    // Issue #29: 履歴が空の undo/redo はエラーにせず false を返す。
    let state = state();
    let res = handle_request(&state, &req("POST", "/undo", ""));
    assert_eq!(res.status, 200);
    let value: serde_json::Value = serde_json::from_str(&res.body).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["undone"], false);
    let res = handle_request(&state, &req("POST", "/redo", ""));
    assert_eq!(res.status, 200);
    let value: serde_json::Value = serde_json::from_str(&res.body).unwrap();
    assert_eq!(value["redone"], false);
}
