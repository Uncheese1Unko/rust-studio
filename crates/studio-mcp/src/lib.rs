//! studio-mcp: MCP サーバー（Issue #5、ADR-0004 / ADR-0008）。
//!
//! stdio 上の JSON-RPC 2.0（改行区切り）で MCP の最小サブセットを話す
//! 薄いラッパー。状態の変更はすべてコマンド層（履歴経由、Issue #29）
//! で、このクレートからプロジェクトを直接変更することはない。
//! 依存は serde / serde_json と自前クレート（studio-core、
//! studio-scripts）のみ（ADR-0008 の方針）。スクリプト一覧のような
//! 読み取りも studio-scripts の共有実装に委ね、ラッパー側に
//! ロジックを複製しない（Issue #82）。

use std::io::{BufRead, Write};
use std::sync::Mutex;

use serde_json::{json, Map, Value};
use studio_core::{Command, History, Project};

/// MCP のプロトコルバージョン（このサーバーが名乗る版）。
const PROTOCOL_VERSION: &str = "2024-11-05";

/// MCP サーバーの状態。プロジェクトはコマンド層経由でのみ変更される。
pub struct McpServer {
    history: Mutex<History>,
    /// プロジェクトファイル backing（Issue #11、ADR-0010）。
    /// Some のとき、変更系ツールの成功たびにプロジェクトをこのファイルへ
    /// 保存し、起動時に読み込む。GUI（video-editor）が同じファイルを読む
    /// ことで、MCP の編集結果が画面にも現れる。
    project_file: Option<std::path::PathBuf>,
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl McpServer {
    pub fn new() -> Self {
        McpServer {
            history: Mutex::new(History::new(Project::new("untitled"))),
            project_file: None,
        }
    }

    /// プロジェクトファイルに backing されたサーバーを作る。
    /// ファイルがあれば読み込む。**読み込みに失敗した場合は backing を
    /// 無効化する**（Issue #11 レビュー対応）: 空プロジェクトで始めて
    /// そのまま保存すると、元のファイルを消してしまうため。GUI 側の
    /// `initial_project` と同じ安全策。
    pub fn with_project_file(path: std::path::PathBuf) -> Self {
        if path.exists() {
            match Project::load_from_path(&path) {
                Ok(project) => McpServer {
                    history: Mutex::new(History::new(project)),
                    project_file: Some(path),
                },
                Err(e) => {
                    eprintln!(
                        "warning: cannot load project file {}: {e}; \
                         starting empty with file backing DISABLED (the file is left untouched)",
                        path.display()
                    );
                    McpServer {
                        history: Mutex::new(History::new(Project::new("untitled"))),
                        project_file: None,
                    }
                }
            }
        } else {
            McpServer {
                history: Mutex::new(History::new(Project::new("untitled"))),
                project_file: Some(path),
            }
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
        if name == "import_midi" {
            // Issue #70: SMF ファイルの取り込み。変換と適用は
            // studio-core 側（失敗時は状態も履歴も変わらない）。
            let path = arguments
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or("import_midi requires a string 'path' argument")?;
            let bytes =
                std::fs::read(path).map_err(|e| format!("cannot read MIDI file '{path}': {e}"))?;
            let mut history = self.history.lock().unwrap();
            match studio_core::import_midi(&mut history, &bytes) {
                Ok(summary) => {
                    let mut payload = json!({
                        "ok": true,
                        "tracks_added": summary.tracks_added,
                        "notes_added": summary.notes_added,
                        "bpm": summary.bpm,
                    });
                    self.persist(&history, &mut payload);
                    return Ok(serde_json::to_string(&payload).unwrap());
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        if name == "get_project" {
            let history = self.history.lock().unwrap();
            return Ok(history.project().dump_json());
        }
        if name == "list_scripts" {
            // Issue #82: video-editor のスクリプト一覧（読み取り専用）。
            // 走査・状態・参照の判定は共有クレート studio-scripts の
            // 単一実装を使い、GUI のダンプとまったく同じ形で返す
            // （#82 レビュー round 1 指摘: ラッパー側に走査ロジックを
            // 複製しない）。置き場所の規約だけはプロセスをまたぐため
            // アプリ側 media::scripts_dir と同じ既定値をここにも持つ。
            // 参照状態は、このサーバーが保持するプロジェクトから
            // 判定する（プロジェクトファイル backing 時は GUI と
            // 同じプロジェクトを見る）。
            let dir = arguments
                .get("dir")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from)
                .unwrap_or_else(default_scripts_dir);
            let entries = studio_scripts::scan_scripts(&dir);
            let history = self.history.lock().unwrap();
            let references = studio_scripts::reference_statuses(history.project(), &dir);
            return Ok(studio_scripts::dump_json(&entries, &references));
        }
        if name == "undo" || name == "redo" {
            // Issue #29: 履歴操作。履歴が空でもエラーにせず、実施できたか
            // どうかを bool で返す（何も起きない結果）。
            let mut history = self.history.lock().unwrap();
            let done = if name == "undo" {
                history.undo()
            } else {
                history.redo()
            };
            let key = if name == "undo" { "undone" } else { "redone" };
            let mut payload = json!({ "ok": true, key: done });
            self.persist(&history, &mut payload);
            return Ok(serde_json::to_string(&payload).unwrap());
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
        let mut history = self.history.lock().unwrap();
        match history.apply(&command) {
            Ok(created_id) => {
                let mut payload = json!({
                    "ok": true,
                    "created_id": created_id,
                });
                self.persist(&history, &mut payload);
                Ok(serde_json::to_string(&payload).unwrap())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// プロジェクトファイル backing 時は、変更後の状態をファイルへ
    /// 反映する（ADR-0010）。保存に失敗しても変更自体は取り消せない
    /// が、結果に `persisted: false` を含めて黙って成功と偽らない
    /// （Issue #11 レビュー対応）。
    fn persist(&self, history: &History, payload: &mut Value) {
        if let Some(path) = &self.project_file {
            match history.project().save_to_path(path) {
                Ok(()) => payload["persisted"] = json!(true),
                Err(e) => {
                    eprintln!("warning: cannot save project file {}: {e}", path.display());
                    payload["persisted"] = json!(false);
                }
            }
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

/// video-editor のスクリプトフォルダの既定位置。アプリ側
/// `media::media_dir` / `media::scripts_dir` と同じ規約（Issue #82）。
/// 走査そのものは studio-scripts の共有実装が行う。
fn default_scripts_dir() -> std::path::PathBuf {
    std::env::temp_dir()
        .join("rust-studio-video-editor-media")
        .join("scripts")
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

/// tools/list で公開する定義。コマンド26種＋MIDI 取り込み＋状態ダンプ取得＋スクリプト一覧＋undo/redo。
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
            "name": "rename_track",
            "description": "トラックの名前を変更する（空白のみの名前はエラー）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "track_id": id_prop("トラック ID"),
                    "name": {"type": "string"},
                },
                "required": ["track_id", "name"],
            },
        },
        {
            "name": "set_track_color",
            "description": "トラックの色を固定パレットから設定する（color を null にすると「なし」に戻す）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "track_id": id_prop("トラック ID"),
                    "color": {"type": ["string", "null"], "enum": ["pink", "coral", "peach", "lemon", "mint", "teal", "sky", "lavender", null]},
                },
                "required": ["track_id"],
            },
        },
        {
            "name": "set_clip_color",
            "description": "クリップの色を固定パレットから設定する（color を null にすると「なし」に戻す）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "color": {"type": ["string", "null"], "enum": ["pink", "coral", "peach", "lemon", "mint", "teal", "sky", "lavender", null]},
                },
                "required": ["clip_id"],
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
            "name": "split_clip",
            "description": "クリップを指定時刻で2つに分割する（右クリップの offset_ms が分割点まで進む）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "at_ms": {"type": "integer", "minimum": 0},
                },
                "required": ["clip_id", "at_ms"],
            },
        },
        {
            "name": "set_clip_offset",
            "description": "クリップの素材内開始位置（offset_ms、ミリ秒、0 = 素材の先頭）を設定する",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "offset_ms": {"type": "integer", "minimum": 0},
                },
                "required": ["clip_id", "offset_ms"],
            },
        },
        {
            "name": "set_track_instrument",
            "description": "トラックの音色（楽器）を設定する（instrument を null にすると楽器なしに戻す）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "track_id": id_prop("トラック ID"),
                    "instrument": {
                        "type": ["object", "null"],
                        "description": "楽器。サンプラーは {\"type\": \"sampler\", \"source\": \"素材パス\", \"root_note\": 60（素材の原音の MIDI ノート番号）}",
                    },
                },
                "required": ["track_id"],
            },
        },
        {
            "name": "add_tone",
            "description": "音色（名前付きの楽器）をプロジェクトに追加する。戻り値は新しい音色の ID",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "音色の名前"},
                    "instrument": {
                        "type": "object",
                        "description": "楽器。サンプラーは {\"type\": \"sampler\", \"source\": \"素材パス\", \"root_note\": 60}、プラグインは {\"type\": \"plugin\", \"format\": \"clap\", \"plugin_path\": \"...\", \"preset_path\": null}",
                    },
                },
                "required": ["name", "instrument"],
            },
        },
        {
            "name": "rename_tone",
            "description": "音色の名前を変える",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tone_id": id_prop("音色 ID"),
                    "name": {"type": "string", "description": "新しい名前"},
                },
                "required": ["tone_id", "name"],
            },
        },
        {
            "name": "update_tone",
            "description": "音色の中身（楽器）を差し替える。割り当て中の全トラックに即座に反映される",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tone_id": id_prop("音色 ID"),
                    "instrument": {
                        "type": "object",
                        "description": "新しい楽器（add_tone と同じ形式）",
                    },
                },
                "required": ["tone_id", "instrument"],
            },
        },
        {
            "name": "remove_tone",
            "description": "音色を削除する。参照していたトラックの割り当ては外れる",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tone_id": id_prop("音色 ID"),
                },
                "required": ["tone_id"],
            },
        },
        {
            "name": "set_track_tone",
            "description": "トラックへ音色を割り当てる（tone_id を null にすると割り当てを外す）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "track_id": id_prop("トラック ID"),
                    "tone_id": {
                        "type": ["integer", "null"],
                        "description": "音色 ID（null で解除）",
                    },
                },
                "required": ["track_id", "tone_id"],
            },
        },
        {
            "name": "add_note",
            "description": "クリップにノートを1つ追加する（音高は MIDI ノート番号、開始はクリップ相対。ノートはクリップの範囲内に収まること）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "pitch": {"type": "integer", "minimum": 0, "maximum": 127},
                    "start_ms": {"type": "integer", "minimum": 0},
                    "duration_ms": {"type": "integer", "minimum": 1},
                    "velocity": {"type": "integer", "minimum": 0, "maximum": 127},
                },
                "required": ["clip_id", "pitch", "start_ms", "duration_ms", "velocity"],
            },
        },
        {
            "name": "move_note",
            "description": "クリップ内のノートを移動する（音高と開始位置。長さは変わらない。移動後もクリップの範囲内に収まること）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "note_index": {"type": "integer", "minimum": 0},
                    "new_pitch": {"type": "integer", "minimum": 0, "maximum": 127},
                    "new_start_ms": {"type": "integer", "minimum": 0},
                },
                "required": ["clip_id", "note_index", "new_pitch", "new_start_ms"],
            },
        },
        {
            "name": "remove_note",
            "description": "クリップ内のノートを1つ削除する（インデックス指定）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "note_index": {"type": "integer", "minimum": 0},
                },
                "required": ["clip_id", "note_index"],
            },
        },
        {
            "name": "set_clip_fade",
            "description": "クリップのフェードイン／フェードアウトの長さ（ミリ秒、直線包絡）を設定する",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "fade_in_ms": {"type": "integer", "minimum": 0},
                    "fade_out_ms": {"type": "integer", "minimum": 0},
                },
                "required": ["clip_id", "fade_in_ms", "fade_out_ms"],
            },
        },
        {
            "name": "set_bpm",
            "description": "プロジェクトのテンポ（BPM、20〜300）を設定する",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "bpm": {"type": "number", "minimum": 20, "maximum": 300},
                },
                "required": ["bpm"],
            },
        },
        {
            "name": "set_clip_cutout",
            "description": "クリップの人物切り抜き適用を切り替える",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "enabled": {"type": "boolean"},
                },
                "required": ["clip_id", "enabled"],
            },
        },
        {
            "name": "set_clip_pitch",
            "description": "クリップのピッチ変更量（半音、-24〜24）を設定する",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "semitones": {"type": "number", "minimum": -24, "maximum": 24},
                },
                "required": ["clip_id", "semitones"],
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
            "name": "set_clip_title",
            "description": "クリップのタイトル文字列を設定する（空文字で消去、Issue #50）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "title": {"type": "string"},
                },
                "required": ["clip_id", "title"],
            },
        },
        {
            "name": "set_clip_script_effect",
            "description": "クリップの Lua スクリプトエフェクトを設定する（effect は {script, params}、null で解除、Issue #81）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "effect": {
                        "type": ["object", "null"],
                        "properties": {
                            "script": {"type": "string"},
                            "params": {"type": "object"},
                        },
                        "required": ["script"],
                    },
                },
                "required": ["clip_id", "effect"],
            },
        },
        {
            "name": "set_clip_transition",
            "description": "クリップの先頭境界のトランジションを設定する（kind: none / cross_dissolve、none は duration_ms=0 で解除、Issue #49）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clip_id": id_prop("クリップ ID"),
                    "kind": {"type": "string", "enum": ["none", "cross_dissolve"]},
                    "duration_ms": {"type": "integer"},
                },
                "required": ["clip_id", "kind", "duration_ms"],
            },
        },
        {
            "name": "set_track_volume",
            "description": "トラックの音量を設定する（0.0〜2.0、既定 1.0）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "track_id": id_prop("トラック ID"),
                    "volume": {"type": "number"},
                },
                "required": ["track_id", "volume"],
            },
        },
        {
            "name": "set_track_pan",
            "description": "トラックのパンを設定する（-1.0=左〜1.0=右、既定 0.0）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "track_id": id_prop("トラック ID"),
                    "pan": {"type": "number"},
                },
                "required": ["track_id", "pan"],
            },
        },
        {
            "name": "set_track_mute",
            "description": "トラックのミュートを設定する（Issue #39）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "track_id": id_prop("トラック ID"),
                    "muted": {"type": "boolean"},
                },
                "required": ["track_id", "muted"],
            },
        },
        {
            "name": "set_track_solo",
            "description": "トラックのソロを設定する（Issue #39、複数可）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "track_id": id_prop("トラック ID"),
                    "solo": {"type": "boolean"},
                },
                "required": ["track_id", "solo"],
            },
        },
        {
            "name": "import_midi",
            "description": "MIDI ファイル（SMF format 0/1）を読み込んでノート列として取り込む（トラックごとに DAW トラックを作成し、テンポはプロジェクトの bpm に反映。取り込みは Undo 1回で戻せる）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "SMF ファイルのパス"},
                },
                "required": ["path"],
            },
        },
        {
            "name": "get_project",
            "description": "プロジェクトの状態ダンプ（JSON）を返す",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "list_scripts",
            "description": "video-editor のスクリプトフォルダにあるスクリプトの一覧（名前・有効／無効・構文状態）と、プロジェクトの参照状態（欠落を含む）を返す。GUI のダンプと同じ形。読み取り専用（Issue #82）",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dir": {"type": "string", "description": "スクリプトフォルダ（省略時はアプリの既定フォルダ）"},
                },
            },
        },
        {
            "name": "undo",
            "description": "直前のコマンド適用を取り消す（履歴が空なら何も起きない）",
            "inputSchema": {"type": "object", "properties": {}},
        },
        {
            "name": "redo",
            "description": "undo で取り消した適用をやり直す（redo がなければ何も起きない）",
            "inputSchema": {"type": "object", "properties": {}},
        },
    ])
}
