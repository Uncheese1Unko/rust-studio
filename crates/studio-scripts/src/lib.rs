//! studio-scripts: Lua スクリプト基盤とスクリプトカタログ（EPIC #78）。
//!
//! `engine` はサンドボックス付き Lua 実行基盤（Issue #80／#81）、
//! `catalog` はスクリプトフォルダの走査・一覧・有効／無効・参照状態
//! （Issue #82）。video-editor（GUI／適用経路）と studio-mcp
//! （list_scripts ツール）が同じ実装を共有し、一覧の形が経路で
//! ずれないようにする（#82 レビュー round 1 指摘への対応）。
//! 状態の変更は扱わない（プロジェクト変更は studio-core の
//! コマンド層のみ。有効／無効はアプリ側設定ファイルへの記録）。

pub mod aviutl;
pub mod catalog;
pub mod engine;

pub use aviutl::{
    check_anm_syntax, decode_sjis, encode_sjis, parse_anm, run_anm_effect, split_reference,
    AnmFile, AnmSection, DialogKind, DialogVar, RgbaImage, TrackDef,
};
pub use catalog::{
    default_scripts_dir, display_rows, dump_json, is_enabled, read_disabled, reference_statuses,
    resolve_script_in, scan_scripts, set_enabled, ReferenceStatus, ScriptEntry, ScriptReference,
    ScriptRow, ScriptRowKind, ScriptStatus, DISABLED_FILE_NAME, SCRIPT_EXTENSIONS,
};
pub use engine::{
    ClipInfo, ScriptEngine, ScriptError, ScriptErrorKind, ScriptParam, ScriptParams, ScriptValue,
    DEFAULT_INSTRUCTION_LIMIT,
};
