//! Issue #5 の受け入れ条件に対応するテスト。
//! MCP（stdio の JSON-RPC）越しにコマンド群が tool として見え、
//! 呼び出すと実行され、状態ダンプに反映されること。

use serde_json::{json, Value};
use studio_mcp::McpServer;

fn rpc(server: &McpServer, request: Value) -> Value {
    let line = serde_json::to_string(&request).unwrap();
    let response = server
        .handle_line(&line)
        .expect("リクエストには応答があるはず");
    serde_json::from_str(&response).unwrap()
}

#[test]
fn initialize_returns_server_info() {
    let server = McpServer::new();
    let res = rpc(
        &server,
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}),
    );
    assert_eq!(res["id"], 1);
    assert!(res["result"]["protocolVersion"].is_string());
    assert_eq!(res["result"]["serverInfo"]["name"], "studio-mcp");
    assert!(res["result"]["capabilities"]["tools"].is_object());
}

#[test]
fn notifications_get_no_response() {
    let server = McpServer::new();
    let line = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    assert_eq!(server.handle_line(line), None);
}

#[test]
fn tools_list_exposes_all_commands() {
    let server = McpServer::new();
    let res = rpc(
        &server,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    );
    let tools = res["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in [
        "add_track",
        "remove_track",
        "add_clip",
        "move_clip",
        "trim_clip",
        "remove_clip",
        "set_clip_gain",
        "set_track_volume",
        "set_track_pan",
        "set_clip_cutout",
        "set_clip_pitch",
        "get_project",
    ] {
        assert!(
            names.contains(&expected),
            "tool {expected} がない: {names:?}"
        );
    }
    // 各 tool に入力スキーマがあること
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object", "tool: {tool}");
    }
}

fn call_tool(server: &McpServer, id: u64, name: &str, arguments: Value) -> Value {
    rpc(
        server,
        json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
               "params": {"name": name, "arguments": arguments}}),
    )
}

fn project_dump_via_tool(server: &McpServer) -> String {
    let res = call_tool(server, 99, "get_project", json!({}));
    assert_eq!(res["result"]["isError"], false);
    res["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn tool_calls_execute_and_dump_reflects_them() {
    let server = McpServer::new();
    let res = call_tool(
        &server,
        1,
        "add_track",
        json!({"name": "V1", "kind": "video"}),
    );
    assert_eq!(res["result"]["isError"], false);
    let text: Value =
        serde_json::from_str(res["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["ok"], true);
    let track_id = text["created_id"].as_u64().unwrap();

    let res = call_tool(
        &server,
        2,
        "add_clip",
        json!({"track_id": track_id, "name": "op", "start_ms": 0,
               "duration_ms": 1500, "source": "media/op.mp4"}),
    );
    assert_eq!(res["result"]["isError"], false);

    // 状態ダンプ（get_project）に反映されていること
    let dump = project_dump_via_tool(&server);
    let dump_json: Value = serde_json::from_str(&dump).unwrap();
    assert_eq!(dump_json["tracks"][0]["name"], "V1");
    assert_eq!(dump_json["tracks"][0]["clips"][0]["name"], "op");

    // API ダンプとの突き合わせ: 同じコマンド列をコマンド層に直接適用した
    // プロジェクトの dump_json と完全一致すること（HTTP API は dump_json を
    // そのまま返すので、これが API ダンプとの同一性の確認になる）。
    let mut expected = studio_core::Project::new("untitled");
    for cmd_json in [
        json!({"type": "add_track", "name": "V1", "kind": "video"}),
        json!({"type": "add_clip", "track_id": track_id, "name": "op",
               "start_ms": 0, "duration_ms": 1500, "source": "media/op.mp4"}),
    ] {
        let cmd: studio_core::Command = serde_json::from_value(cmd_json).unwrap();
        studio_core::apply(&mut expected, &cmd).unwrap();
    }
    assert_eq!(dump, expected.dump_json());
}

#[test]
fn failing_tool_call_errors_and_leaves_state_unchanged() {
    let server = McpServer::new();
    call_tool(
        &server,
        1,
        "add_track",
        json!({"name": "V1", "kind": "video"}),
    );
    let before = project_dump_via_tool(&server);

    // 存在しないトラックへの add_clip はエラーになり、状態は変わらない
    let res = call_tool(
        &server,
        2,
        "add_clip",
        json!({"track_id": 999, "name": "x", "start_ms": 0,
               "duration_ms": 100, "source": "x.mp4"}),
    );
    assert_eq!(res["result"]["isError"], true);
    assert!(res["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("999"));

    // 引数の型が違う場合もエラーで、状態は変わらない
    let res = call_tool(
        &server,
        3,
        "add_track",
        json!({"name": 42, "kind": "video"}),
    );
    assert_eq!(res["result"]["isError"], true);

    assert_eq!(project_dump_via_tool(&server), before);
}

#[test]
fn unknown_tool_and_unknown_method_error() {
    let server = McpServer::new();
    let res = call_tool(&server, 1, "no_such_tool", json!({}));
    assert_eq!(res["result"]["isError"], true);

    let res = rpc(
        &server,
        json!({"jsonrpc": "2.0", "id": 7, "method": "no/such", "params": {}}),
    );
    assert_eq!(res["id"], 7);
    assert_eq!(res["error"]["code"], -32601);
}

#[test]
fn malformed_line_returns_parse_error() {
    let server = McpServer::new();
    let response = server.handle_line("{ not json").unwrap();
    let res: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(res["error"]["code"], -32700);
}

#[test]
fn stdio_end_to_end_session() {
    // 実際のバイナリを起動し、改行区切り JSON-RPC で1セッション行う
    let exe = env!("CARGO_BIN_EXE_studio-mcp");
    let mut child = std::process::Command::new(exe)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#
        )
        .unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
        )
        .unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"add_track","arguments":{{"name":"A1","kind":"audio"}}}}}}"#
        )
        .unwrap();
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"get_project","arguments":{{}}}}}}"#
        )
        .unwrap();
    } // stdin を閉じる → サーバーは EOF で終了する
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<Value> = stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    // 通知には応答がないので、応答は3行（initialize / add_track / get_project）
    assert_eq!(lines.len(), 3, "stdout: {stdout}");
    assert_eq!(lines[0]["id"], 1);
    assert_eq!(lines[1]["result"]["isError"], false);
    let dump_text = lines[2]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(dump_text.contains("\"A1\""), "dump: {dump_text}");
}

#[test]
fn trim_clip_tool_applies_via_tools_call() {
    let server = McpServer::new();
    let res = call_tool(
        &server,
        10,
        "add_track",
        json!({"name": "V1", "kind": "video"}),
    );
    assert_eq!(res["result"]["isError"], false);
    let res = call_tool(
        &server,
        11,
        "add_clip",
        json!({"track_id": 1, "name": "opening", "start_ms": 1000,
               "duration_ms": 2000, "source": "demo/opening.mp4"}),
    );
    assert_eq!(res["result"]["isError"], false);
    // 配置したクリップ（id は採番順で 2）をトリミングする
    let res = call_tool(
        &server,
        12,
        "trim_clip",
        json!({"clip_id": 2, "new_start_ms": 1300, "new_duration_ms": 1700}),
    );
    assert_eq!(res["result"]["isError"], false, "{res}");
    let dump = project_dump_via_tool(&server);
    let project: studio_core::Project = serde_json::from_str(&dump).unwrap();
    let (_, clip) = project.clip(2).unwrap();
    assert_eq!(clip.start_ms, 1300);
    assert_eq!(clip.duration_ms, 1700);
}

#[test]
fn project_file_backing_persists_across_server_instances() {
    // Issue #11: --project-file モードでは、コマンドの結果がファイルに
    // 保存され、別のサーバーインスタンス（= GUI と同じファイルを読む側）
    // から同じ状態が見えること。
    let path =
        std::env::temp_dir().join(format!("rust-studio-mcp-test-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);

    let server = studio_mcp::McpServer::with_project_file(path.clone());
    let res = call_tool(
        &server,
        20,
        "add_track",
        json!({"name": "V1", "kind": "video"}),
    );
    assert_eq!(res["result"]["isError"], false);
    let res = call_tool(
        &server,
        21,
        "add_clip",
        json!({"track_id": 1, "name": "opening", "start_ms": 0,
               "duration_ms": 2000, "source": "demo/opening.mp4"}),
    );
    assert_eq!(res["result"]["isError"], false);
    let dump_a = project_dump_via_tool(&server);
    drop(server);

    // ファイルが存在し、内容がダンプと一致する
    let on_disk = std::fs::read_to_string(&path).unwrap();
    let from_disk: studio_core::Project = serde_json::from_str(&on_disk).unwrap();
    let from_dump: studio_core::Project = serde_json::from_str(&dump_a).unwrap();
    assert_eq!(from_disk, from_dump);

    // 別のインスタンスは同じ状態から始まり、移動もまた保存される
    let server_b = studio_mcp::McpServer::with_project_file(path.clone());
    assert_eq!(project_dump_via_tool(&server_b), dump_a);
    let res = call_tool(
        &server_b,
        22,
        "move_clip",
        json!({"clip_id": 2, "new_start_ms": 1000}),
    );
    assert_eq!(res["result"]["isError"], false);
    drop(server_b);
    let on_disk = std::fs::read_to_string(&path).unwrap();
    let project: studio_core::Project = serde_json::from_str(&on_disk).unwrap();
    let (_, clip) = project.clip(2).unwrap();
    assert_eq!(clip.start_ms, 1000);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn project_file_load_failure_disables_backing_and_preserves_file() {
    // Issue #11 レビュー対応: 読み込めないファイルで起動した場合、
    // 空プロジェクトで始めても元のファイルを上書きしてはならない
    // （backing を無効化する。GUI 側 initial_project と同じ安全策）。
    let path = std::env::temp_dir().join(format!(
        "rust-studio-mcp-broken-{}.json",
        std::process::id()
    ));
    let broken = "{\"name\": \"大切なプロジェクト\", \"tracks\": [";
    std::fs::write(&path, broken).unwrap();

    let server = studio_mcp::McpServer::with_project_file(path.clone());
    let res = call_tool(
        &server,
        30,
        "add_track",
        json!({"name": "V1", "kind": "video"}),
    );
    // メモリ上では操作できるが、ファイルは元のまま残る
    assert_eq!(res["result"]["isError"], false);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn project_file_save_failure_is_reported_in_result() {
    // 保存できない場所を backing にした場合、結果に persisted: false が
    // 含まれ、成功と偽らないこと。
    let path = std::env::temp_dir()
        .join("rust-studio-no-such-dir-xyz")
        .join("project.json");
    let server = studio_mcp::McpServer::with_project_file(path);
    let res = call_tool(
        &server,
        31,
        "add_track",
        json!({"name": "V1", "kind": "video"}),
    );
    assert_eq!(res["result"]["isError"], false);
    let text = res["result"]["content"][0]["text"].as_str().unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(payload["persisted"], false);
}
