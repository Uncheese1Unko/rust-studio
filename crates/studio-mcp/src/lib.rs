//! studio-mcp: MCP サーバー（Issue #5、ADR-0004 / ADR-0008）。
//!
//! stdio 上の JSON-RPC 2.0（改行区切り）で MCP の最小サブセットを話す
//! 薄いラッパー。状態の変更はすべてコマンド層（`studio_core::apply`）
//! 経由で、このクレートからプロジェクトを直接変更することはない。
//! 依存は serde / serde_json のみ（ADR-0008 の方針）。

use std::io::{BufRead, Write};
use std::sync::Mutex;

use serde_json::{json, Map, Value};
use studio_core::{Command, Project};

/// MCP のプロトコルバージョン（このサーバーが名乗る版）。
const PROTOCOL_VERSION: &str = "2024-11-05";

/// MCP サーバーの状態。プロジェクトはコマンド層経由でのみ変更される。
pub struct McpServer {
    project: Mutex<Project>,
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl McpServer {
    pub fn new() -> Self {
        McpServer {
            project: Mutex::new(Project::new("untitled")),
        }
    }

    /// JSON-RPC の1行を受け取り、応答行を返す。
    /// 通知（id なし）には応答しないので `None` を返す。
    pub fn handle_line(&self, line: &str) -> Option<String> {
        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(error_response(
                    Value::Null,
                    -32700,
                    &format!("parse error: {e}"),
                ));
            }
        };
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(Value::Null);

        let result = match method {
            "initialize" => json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "studio-mcp", "version": env!("CARGO_PKG_VERSION")},
            }),
            "ping" => json!({}),
            "tools/list" => json!({"tools": tool_definitions()}),
            "tools/call" => self.call_tool(&params),
            _ => {
                return id
                    .map(|id| error_response(id, -32601, &format!("method not found: {method}")));
            }
        };

        id.map(|id| {
            let response = json!({"jsonrpc": "2.0", "id": id, "result": result});
            serde_json::to_string(&response).unwrap()
        })
    }

    /// tools/call の処理。tool のエラーは JSON-RPC エラーではなく
    /// `isError: true` の結果として返す（MCP の慣例）。
    fn call_tool(&self, params: &Value) -> Value {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        match self.execute_tool(name, &arguments) {
            Ok(text) => tool_result(text, false),
            Err(message) => tool_result(message, true),
        }
    }

    fn execute_tool(&self, name: &str, arguments: &Value) -> Result<String, String> {
        if name == "get_project" {
            let project = self.project.lock().unwrap();
            return Ok(project.dump_json());
        }
        // tool 名 = コマンドの type タグ。引数オブジェクトに type を足して
        // コマンドとして解釈する（未知の tool 名はここで弾かれる）。
        let mut object = match arguments {
            Value::Object(map) => map.clone(),
            Value::Null => Map::new(),
            _ => return Err("arguments must be a JSON object".to_string()),
        };
        object.insert("type".to_string(), Value::String(name.to_string()));
        let command: Command = serde_json::from_value(Value::Object(object))
            .map_err(|e| format!("invalid arguments for tool '{name}': {e}"))?;
        let mut project = self.project.lock().unwrap();
        match studio_core::apply(&mut project, &command) {
            Ok(created_id) => Ok(serde_json::to_string(&json!({
                "ok": true,
                "created_id": created_id,
            }))
            .unwrap()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// stdin / stdout で MCP セッションを処理する（EOF で終了）。
    pub fn serve_stdio<R: BufRead, W: Write>(
        &self,
        reader: R,
        mut writer: W,
    ) -> std::io::Result<()> {
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(response) = self.handle_line(&line) {
                writeln!(writer, "{response}")?;
                writer.flush()?;
            }
        }
        Ok(())
    }
}

fn tool_result(text: String, is_error: bool) -> Value {
    json!({
        "content": [{"type": "text", "text": text}],
        "isError": is_error,
    })
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message},
    }))
    .unwrap()
}

/// tools/list で公開する定義。コマンド7種＋状態ダンプ取得。
fn tool_definitions() -> Value {
    let id_prop = |desc: &str| json!({"type": "integer", "description": desc});
    json!([
        {
            "name": "add_track",
            "description": "トラックを追加する",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "kind": {"type": "string", "enum": ["video", "audio"]},
                },
                "required": ["name", "kind"],
            },
        },
        {
            "name": "remove_track",
            "description": "トラックを削除する",
            "inputSchema": {
                "type": "object",
                "properties": {"track_id": id_prop("トラック ID")},
                "required": ["track_id"],
            },
        },
        {
            "name": "add_clip",
            "description": "クリップをトラックに追加する",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "track_id": id_prop("トラック ID"),
                    "name": {"type": "string"},
                    "start_ms": {"type": "integer", "minimum": 0},
                    "duration_ms": {"type": "integer", "minimum": 0},
                    "source": {"type": "string"},
                },
                "required": ["track_id", "name", "start_ms", "duration_ms", "source"],
            },
        },
        {
            "name": "move_clip",
            "description": "クリップの開始位置を移動する",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "new_start_ms": {"type": "integer", "minimum": 0},
                },
                "required": ["clip_id", "new_start_ms"],
            },
        },
        {
            "name": "trim_clip",
            "description": "クリップの開始位置と長さをまとめて変える（トリミング）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "new_start_ms": {"type": "integer", "minimum": 0},
                    "new_duration_ms": {"type": "integer", "minimum": 1},
                },
                "required": ["clip_id", "new_start_ms", "new_duration_ms"],
            },
        },
        {
            "name": "remove_clip",
            "description": "クリップを削除する",
            "inputSchema": {
                "type": "object",
                "properties": {"clip_id": id_prop("クリップ ID")},
                "required": ["clip_id"],
            },
        },
        {
            "name": "set_clip_gain",
            "description": "クリップのゲインを設定する（0.0〜4.0）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "gain": {"type": "number"},
                },
                "required": ["clip_id", "gain"],
            },
        },
        {
            "name": "get_project",
            "description": "プロジェクトの状態ダンプ（JSON）を返す",
            "inputSchema": {"type": "object", "properties": {}},
        },
    ])
}
