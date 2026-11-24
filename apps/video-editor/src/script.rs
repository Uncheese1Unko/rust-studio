//! Lua スクリプト実行基盤（Issue #80）。
//!
//! 動画編集の拡張（EPIC #78）の土台。スクリプトはサンドボックス内で
//! 実行し、ファイル・OS コマンド・ネットワークへは到達できない。
//! ランタイムは ADR-0019 の確定どおり Lua 5.1 系（mlua + vendored）。
//!
//! ここで公開するのは内部 API（`clip` / `params` グローバル）までで、
//! AviUtl `obj.*` の互換形状は #83 で別層として足す。スクリプトから
//! プロジェクト状態を直接書き換える経路は持たない（ADR-0004）。

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use mlua::{HookTriggers, Lua, LuaOptions, StdLib, Value as LuaValue, VmState};

/// フックの発火間隔（命令数）。上限判定はこの粒度で行う。
const HOOK_INTERVAL: u32 = 2048;
/// スクリプトに許すメモリ上限（バイト）。
const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// 命令数上限で打ち切ったときにフックが投げる目印メッセージ。
const LIMIT_MESSAGE: &str = "script instruction limit exceeded";

/// 既定の命令数上限（暴走スクリプトの打ち切り）。
pub const DEFAULT_INSTRUCTION_LIMIT: u64 = 10_000_000;

/// スクリプトへ渡す対象クリップの情報（最小 API の読み取り側）。
#[derive(Debug, Clone, PartialEq)]
pub struct ClipInfo {
    pub name: String,
    pub duration_ms: i64,
    pub source: Option<String>,
}

/// パラメータ値（数値・真偽・文字列）。
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptParam {
    Number(f64),
    Bool(bool),
    Text(String),
}

/// スクリプトへ渡すパラメータ集合。
pub type ScriptParams = BTreeMap<String, ScriptParam>;

/// スクリプトの戻り値（スカラーのみを構造化して返す）。
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptValue {
    Nil,
    Bool(bool),
    Number(f64),
    Text(String),
}

/// 構造化エラーの種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptErrorKind {
    /// スクリプトファイルが読めない（`run_file` のみ）。
    Io,
    /// 構文エラー。
    Syntax,
    /// 実行時エラー（サンドボックス到達の試みを含む）。
    Runtime,
    /// 命令数上限による打ち切り。
    InstructionLimit,
}

/// 構造化エラー。呼び出し側はアプリを落とさずに表示・記録できる。
#[derive(Debug, Clone, PartialEq)]
pub struct ScriptError {
    pub kind: ScriptErrorKind,
    pub message: String,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for ScriptError {}

/// スクリプト実行エンジン。Lua 状態は実行ごとに作り、持ち回さない。
pub struct ScriptEngine {
    instruction_limit: u64,
}

impl Default for ScriptEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptEngine {
    pub fn new() -> Self {
        Self {
            instruction_limit: DEFAULT_INSTRUCTION_LIMIT,
        }
    }

    /// 命令数上限を指定する（テスト・用途別の調整用）。
    pub fn with_instruction_limit(instruction_limit: u64) -> Self {
        Self { instruction_limit }
    }

    /// スクリプトファイルを読んで実行する。
    pub fn run_file(
        &self,
        path: &Path,
        clip: &ClipInfo,
        params: &ScriptParams,
    ) -> Result<ScriptValue, ScriptError> {
        let source = std::fs::read_to_string(path).map_err(|e| ScriptError {
            kind: ScriptErrorKind::Io,
            message: format!("{}: {e}", path.display()),
        })?;
        self.run_source(&source, clip, params)
    }

    /// スクリプト文字列を実行し、戻り値か構造化エラーを返す。
    pub fn run_source(
        &self,
        source: &str,
        clip: &ClipInfo,
        params: &ScriptParams,
    ) -> Result<ScriptValue, ScriptError> {
        // サンドボックス: 標準ライブラリは安全な最小集合だけを読み込む。
        // mlua の ALL_SAFE は io/os を含むため使わない。ベースライブラリは
        // 常に読み込まれるので、到達経路になる関数は個別に落とす。
        // - ファイル読み込み系（dofile/loadfile/load/loadstring/require）:
        //   ファイル・モジュールへ到達させない。
        // - エラー捕捉系（pcall/xpcall/coroutine）: 命令数上限のエラーは
        //   通常の Lua エラーのため、捕捉手段があると暴走スクリプトが
        //   打ち切りを回避できてしまう（#80 レビュー round 1 指摘）。
        //   捕捉手段そのものを外し、上限エラーは必ず呼び出し側へ届く
        //   ようにする。スクリプトのエラー処理は構造化エラーとして
        //   ホスト側が担う設計である。
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH,
            LuaOptions::default(),
        )
        .map_err(|e| runtime_error(e.to_string()))?;
        let _ = lua.set_memory_limit(MEMORY_LIMIT);
        let globals = lua.globals();
        for name in [
            "dofile",
            "loadfile",
            "load",
            "loadstring",
            "require",
            "pcall",
            "xpcall",
            "coroutine",
        ] {
            globals
                .set(name, LuaValue::Nil)
                .map_err(|e| runtime_error(e.to_string()))?;
        }

        // 最小 API: clip（対象クリップの情報）と params（パラメータ）。
        let clip_table = lua
            .create_table()
            .map_err(|e| runtime_error(e.to_string()))?;
        clip_table
            .set("name", clip.name.as_str())
            .map_err(|e| runtime_error(e.to_string()))?;
        clip_table
            .set("duration_ms", clip.duration_ms)
            .map_err(|e| runtime_error(e.to_string()))?;
        match &clip.source {
            Some(source) => clip_table
                .set("source", source.as_str())
                .map_err(|e| runtime_error(e.to_string()))?,
            None => clip_table
                .set("source", LuaValue::Nil)
                .map_err(|e| runtime_error(e.to_string()))?,
        }
        globals
            .set("clip", clip_table)
            .map_err(|e| runtime_error(e.to_string()))?;

        let params_table = lua
            .create_table()
            .map_err(|e| runtime_error(e.to_string()))?;
        for (key, value) in params {
            let value = match value {
                ScriptParam::Number(n) => LuaValue::Number(*n),
                ScriptParam::Bool(b) => LuaValue::Boolean(*b),
                ScriptParam::Text(t) => LuaValue::String(
                    lua.create_string(t)
                        .map_err(|e| runtime_error(e.to_string()))?,
                ),
            };
            params_table
                .set(key.as_str(), value)
                .map_err(|e| runtime_error(e.to_string()))?;
        }
        globals
            .set("params", params_table)
            .map_err(|e| runtime_error(e.to_string()))?;

        // 暴走打ち切り: 命令数を数え、上限を超えたらエラーで割り込む。
        let exceeded = Arc::new(AtomicBool::new(false));
        let counter = Arc::new(AtomicU64::new(0));
        let limit = self.instruction_limit;
        let flag = exceeded.clone();
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(HOOK_INTERVAL),
            move |_lua, _debug| {
                let executed = counter.fetch_add(HOOK_INTERVAL as u64, Ordering::Relaxed)
                    + HOOK_INTERVAL as u64;
                if executed > limit {
                    flag.store(true, Ordering::Relaxed);
                    return Err(mlua::Error::runtime(LIMIT_MESSAGE));
                }
                Ok(VmState::Continue)
            },
        )
        .map_err(|e| runtime_error(e.to_string()))?;

        let result = lua.load(source).eval::<LuaValue>();
        lua.remove_hook();
        // 上限超過は成否にかかわらず打ち切りとして返す（捕捉手段を
        // 外してあるため通常は Err で届くが、二重に保証する）。
        if exceeded.load(Ordering::Relaxed) {
            return Err(ScriptError {
                kind: ScriptErrorKind::InstructionLimit,
                message: format!(
                    "命令数上限（{}）を超えたため打ち切りました",
                    self.instruction_limit
                ),
            });
        }
        match result {
            Ok(value) => convert_value(value),
            Err(e) => Err(classify_error(e)),
        }
    }
}

fn runtime_error(message: String) -> ScriptError {
    ScriptError {
        kind: ScriptErrorKind::Runtime,
        message,
    }
}

/// mlua のエラーを構造化エラーへ寄せる。
fn classify_error(error: mlua::Error) -> ScriptError {
    match error {
        mlua::Error::SyntaxError { message, .. } => ScriptError {
            kind: ScriptErrorKind::Syntax,
            message,
        },
        other => runtime_error(other.to_string()),
    }
}

/// 戻り値はスカラーのみ構造化する。それ以外は実行時エラーとして返す。
fn convert_value(value: LuaValue) -> Result<ScriptValue, ScriptError> {
    match value {
        LuaValue::Nil => Ok(ScriptValue::Nil),
        LuaValue::Boolean(b) => Ok(ScriptValue::Bool(b)),
        LuaValue::Integer(i) => Ok(ScriptValue::Number(i as f64)),
        LuaValue::Number(n) => Ok(ScriptValue::Number(n)),
        LuaValue::String(s) => Ok(ScriptValue::Text(s.to_string_lossy())),
        other => Err(runtime_error(format!(
            "スクリプトの戻り値はスカラー（nil／真偽／数値／文字列）のみ対応です: {}",
            other.type_name()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip() -> ClipInfo {
        ClipInfo {
            name: "clip-a".to_string(),
            duration_ms: 5000,
            source: Some("media/a.mp4".to_string()),
        }
    }

    fn no_params() -> ScriptParams {
        ScriptParams::new()
    }

    #[test]
    fn returns_number_result() {
        let engine = ScriptEngine::new();
        assert_eq!(
            engine.run_source("return 1 + 2", &clip(), &no_params()),
            Ok(ScriptValue::Number(3.0))
        );
    }

    #[test]
    fn returns_text_bool_and_nil_results() {
        let engine = ScriptEngine::new();
        assert_eq!(
            engine.run_source("return 'abc'", &clip(), &no_params()),
            Ok(ScriptValue::Text("abc".to_string()))
        );
        assert_eq!(
            engine.run_source("return true", &clip(), &no_params()),
            Ok(ScriptValue::Bool(true))
        );
        assert_eq!(
            engine.run_source("return nil", &clip(), &no_params()),
            Ok(ScriptValue::Nil)
        );
    }

    #[test]
    fn syntax_error_is_structured() {
        let engine = ScriptEngine::new();
        let err = engine
            .run_source("return 1 +", &clip(), &no_params())
            .unwrap_err();
        assert_eq!(err.kind, ScriptErrorKind::Syntax);
    }

    #[test]
    fn runtime_error_is_structured() {
        let engine = ScriptEngine::new();
        let err = engine
            .run_source("error('boom')", &clip(), &no_params())
            .unwrap_err();
        assert_eq!(err.kind, ScriptErrorKind::Runtime);
        assert!(err.message.contains("boom"), "message: {}", err.message);
    }

    #[test]
    fn sandbox_has_no_os_io_package_debug() {
        let engine = ScriptEngine::new();
        assert_eq!(
            engine.run_source(
                "return os == nil and io == nil and package == nil and debug == nil",
                &clip(),
                &no_params()
            ),
            Ok(ScriptValue::Bool(true))
        );
    }

    #[test]
    fn sandbox_blocks_reach_attempts() {
        let engine = ScriptEngine::new();
        for source in [
            "return os.execute('true')",
            "local f = io.open('x', 'r')",
            "return require('os')",
            "return loadfile('x')",
            "return dofile('x')",
            "return load('return 1')()",
        ] {
            assert!(
                engine.run_source(source, &clip(), &no_params()).is_err(),
                "到達できてしまった: {source}"
            );
        }
    }

    #[test]
    fn infinite_loop_is_interrupted_by_instruction_limit() {
        let engine = ScriptEngine::with_instruction_limit(20_000);
        let err = engine
            .run_source("while true do end", &clip(), &no_params())
            .unwrap_err();
        assert_eq!(err.kind, ScriptErrorKind::InstructionLimit);
    }

    #[test]
    fn clip_info_is_exposed() {
        let engine = ScriptEngine::new();
        assert_eq!(
            engine.run_source(
                "return clip.name .. ':' .. clip.duration_ms .. ':' .. clip.source",
                &clip(),
                &no_params()
            ),
            Ok(ScriptValue::Text("clip-a:5000:media/a.mp4".to_string()))
        );
    }

    #[test]
    fn clip_source_is_nil_when_absent() {
        let engine = ScriptEngine::new();
        let mut c = clip();
        c.source = None;
        assert_eq!(
            engine.run_source("return clip.source == nil", &c, &no_params()),
            Ok(ScriptValue::Bool(true))
        );
    }

    #[test]
    fn params_are_exposed() {
        let engine = ScriptEngine::new();
        let mut params = ScriptParams::new();
        params.insert("gain".to_string(), ScriptParam::Number(1.5));
        params.insert("enabled".to_string(), ScriptParam::Bool(true));
        params.insert("label".to_string(), ScriptParam::Text("hi".to_string()));
        assert_eq!(
            engine.run_source("return params.gain + 1", &clip(), &params),
            Ok(ScriptValue::Number(2.5))
        );
        assert_eq!(
            engine.run_source("return params.enabled", &clip(), &params),
            Ok(ScriptValue::Bool(true))
        );
        assert_eq!(
            engine.run_source("return params.label", &clip(), &params),
            Ok(ScriptValue::Text("hi".to_string()))
        );
    }

    #[test]
    fn run_file_executes_script_file() {
        let engine = ScriptEngine::new();
        let path =
            std::env::temp_dir().join(format!("studio-script-{}-ok.lua", std::process::id()));
        std::fs::write(&path, "return 40 + 2").unwrap();
        let result = engine.run_file(&path, &clip(), &no_params());
        let _ = std::fs::remove_file(&path);
        assert_eq!(result, Ok(ScriptValue::Number(42.0)));
    }

    #[test]
    fn run_file_missing_is_io_error() {
        let engine = ScriptEngine::new();
        let path = std::env::temp_dir().join("studio-script-no-such-file.lua");
        let err = engine.run_file(&path, &clip(), &no_params()).unwrap_err();
        assert_eq!(err.kind, ScriptErrorKind::Io);
    }

    #[test]
    fn non_scalar_return_is_error() {
        let engine = ScriptEngine::new();
        let err = engine
            .run_source("return {}", &clip(), &no_params())
            .unwrap_err();
        assert_eq!(err.kind, ScriptErrorKind::Runtime);
    }

    // レビュー指摘（#80 round 1）: 上限エラーが通常の Lua エラーだと
    // pcall で捕捉でき、暴走スクリプトが止まらない。捕捉手段そのものを
    // サンドボックスから外すことで打ち切りを保証する。
    #[test]
    fn sandbox_has_no_error_catching_primitives() {
        let engine = ScriptEngine::new();
        assert_eq!(
            engine.run_source(
                "return pcall == nil and xpcall == nil and coroutine == nil",
                &clip(),
                &no_params()
            ),
            Ok(ScriptValue::Bool(true))
        );
    }

    #[test]
    fn pcall_wrapped_infinite_loop_terminates_with_error() {
        // レビュアーのプローブ。修正前は 300 秒タイムアウトまで終了
        // しなかった。修正後は pcall が無いため即座にエラーで終わる。
        let engine = ScriptEngine::with_instruction_limit(20_000);
        assert!(engine
            .run_source(
                "while true do pcall(function() while true do end end) end",
                &clip(),
                &no_params()
            )
            .is_err());
    }

    #[test]
    fn long_loop_over_limit_is_instruction_limit_not_success() {
        // 上限超過後に正常終了する形でも、成功として返さない。
        let engine = ScriptEngine::with_instruction_limit(10_000);
        let err = engine
            .run_source(
                "for i = 1, 100000000 do end return 42",
                &clip(),
                &no_params(),
            )
            .unwrap_err();
        assert_eq!(err.kind, ScriptErrorKind::InstructionLimit);
    }
}
