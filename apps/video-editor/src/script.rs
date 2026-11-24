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
    /// クリップ内の現在時刻（ミリ秒）。フレームエフェクト用（#81）。
    pub time_ms: i64,
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
        let (lua, exceeded) = self.build_lua(clip, params)?;
        let result = lua.load(source).eval::<LuaValue>();
        lua.remove_hook();
        self.check_limit(&exceeded)?;
        match result {
            Ok(value) => convert_value(value),
            Err(e) => Err(classify_error(e)),
        }
    }

    /// 上限超過なら打ち切りエラーにする。超過は成否にかかわらず
    /// 打ち切りとして返す（捕捉手段を外してあるため通常は Err で
    /// 届くが、二重に保証する）。
    fn check_limit(&self, exceeded: &AtomicBool) -> Result<(), ScriptError> {
        if exceeded.load(Ordering::Relaxed) {
            Err(ScriptError {
                kind: ScriptErrorKind::InstructionLimit,
                message: format!(
                    "命令数上限（{}）を超えたため打ち切りました",
                    self.instruction_limit
                ),
            })
        } else {
            Ok(())
        }
    }

    /// サンドボックス済みの Lua 状態を作り、最小 API（clip / params）
    /// と命令数フックを入れて返す。超過フラグもあわせて返す。
    fn build_lua(
        &self,
        clip: &ClipInfo,
        params: &ScriptParams,
    ) -> Result<(Lua, Arc<AtomicBool>), ScriptError> {
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
        clip_table
            .set("time_ms", clip.time_ms)
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
        Ok((lua, exceeded))
    }

    /// ソースをコンパイルだけして構文を検証する（実行はしない）。
    /// 一覧での「壊れたスクリプト」の区別に使う（Issue #82）。
    pub fn check_syntax(&self, source: &str) -> Result<(), ScriptError> {
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH,
            LuaOptions::default(),
        )
        .map_err(|e| runtime_error(e.to_string()))?;
        match lua.load(source).into_function() {
            Ok(_) => Ok(()),
            Err(e) => Err(classify_error(e)),
        }
    }

    /// フレーム（RGBA8 ピクセル列）へスクリプトエフェクトを適用する
    /// （Issue #81）。スクリプトは `frame` グローバル経由でピクセルを
    /// 読み書きする（内部 API で、AviUtl の `obj.*` ではない。互換
    /// 形状は #83）。戻り値は使わない。
    pub fn run_frame_effect(
        &self,
        source: &str,
        clip: &ClipInfo,
        params: &ScriptParams,
        width: u32,
        height: u32,
        pixels: &mut [u8],
    ) -> Result<(), ScriptError> {
        let expected = (width as usize) * (height as usize) * 4;
        if pixels.len() != expected {
            return Err(runtime_error(format!(
                "フレームバッファの長さが寸法と一致しません: {}x{} には {expected} バイト必要ですが {} バイトです",
                width,
                height,
                pixels.len()
            )));
        }
        let (lua, exceeded) = self.build_lua(clip, params)?;

        // frame グローバル: ピクセル読み書きはスコープ付き関数で
        // バッファを借りる（Lua 状態は実行ごとに捨てる）。
        let cell = std::cell::RefCell::new(pixels);
        let result = lua.scope(|scope| {
            let frame = lua.create_table()?;
            frame.set("width", width)?;
            frame.set("height", height)?;
            let get = scope.create_function(|_, (x, y): (f64, f64)| {
                let buf = cell.borrow();
                let idx = frame_pixel_index(width, height, coord(x), coord(y))?;
                Ok((
                    buf[idx] as i64,
                    buf[idx + 1] as i64,
                    buf[idx + 2] as i64,
                    buf[idx + 3] as i64,
                ))
            })?;
            frame.set("get", get)?;
            let set = scope.create_function(
                |_, (x, y, r, g, b, a): (f64, f64, f64, f64, f64, Option<f64>)| {
                    let mut buf = cell.borrow_mut();
                    let idx = frame_pixel_index(width, height, coord(x), coord(y))?;
                    buf[idx] = channel(r);
                    buf[idx + 1] = channel(g);
                    buf[idx + 2] = channel(b);
                    buf[idx + 3] = channel(a.unwrap_or(255.0));
                    Ok(())
                },
            )?;
            frame.set("set", set)?;
            let fill_rect = scope.create_function(
                |_, (x, y, w, h, r, g, b, a): (f64, f64, f64, f64, f64, f64, f64, Option<f64>)| {
                    let mut buf = cell.borrow_mut();
                    let (x, y, w, h) = (coord(x), coord(y), coord(w), coord(h));
                    // 描画プリミティブはフレーム範囲でクリップする。
                    let x0 = x.max(0);
                    let y0 = y.max(0);
                    let x1 = (x + w).min(width as i64);
                    let y1 = (y + h).min(height as i64);
                    let rgba = [
                        channel(r),
                        channel(g),
                        channel(b),
                        channel(a.unwrap_or(255.0)),
                    ];
                    for yy in y0..y1 {
                        for xx in x0..x1 {
                            let idx = ((yy * width as i64 + xx) * 4) as usize;
                            buf[idx..idx + 4].copy_from_slice(&rgba);
                        }
                    }
                    Ok(())
                },
            )?;
            frame.set("fill_rect", fill_rect)?;
            lua.globals().set("frame", frame)?;
            lua.load(source).eval::<LuaValue>()
        });
        lua.remove_hook();
        self.check_limit(&exceeded)?;
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(classify_error(e)),
        }
    }
}

/// Lua の数値をピクセル座標へ寄せる。
fn coord(value: f64) -> i64 {
    value.round() as i64
}

/// Lua の数値を色チャンネル（0〜255）へ寄せる。
fn channel(value: f64) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

/// フレーム内のピクセル先頭インデックス。範囲外は実行時エラー。
fn frame_pixel_index(width: u32, height: u32, x: i64, y: i64) -> mlua::Result<usize> {
    if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
        return Err(mlua::Error::runtime(format!(
            "frame coordinate out of bounds: ({x}, {y}) for {width}x{height}"
        )));
    }
    Ok(((y * width as i64 + x) * 4) as usize)
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
            time_ms: 0,
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

    // ---- フレームエフェクト（Issue #81） ----

    #[test]
    fn clip_time_is_exposed() {
        let engine = ScriptEngine::new();
        let mut c = clip();
        c.time_ms = 1500;
        assert_eq!(
            engine.run_source("return clip.time_ms", &c, &no_params()),
            Ok(ScriptValue::Number(1500.0))
        );
    }

    #[test]
    fn frame_effect_inverts_pixels() {
        let engine = ScriptEngine::new();
        // 2x1 の RGBA: (10,20,30,255) (200,150,100,255)
        let mut pixels = vec![10, 20, 30, 255, 200, 150, 100, 255];
        engine
            .run_frame_effect(
                "for x = 0, frame.width - 1 do
                     local r, g, b, a = frame.get(x, 0)
                     frame.set(x, 0, 255 - r, 255 - g, 255 - b, a)
                 end",
                &clip(),
                &no_params(),
                2,
                1,
                &mut pixels,
            )
            .unwrap();
        assert_eq!(pixels, vec![245, 235, 225, 255, 55, 105, 155, 255]);
    }

    #[test]
    fn frame_effect_fill_rect_draws_and_clips() {
        let engine = ScriptEngine::new();
        let mut pixels = vec![0u8; 4 * 4 * 4];
        engine
            .run_frame_effect(
                // 範囲をはみ出す矩形はクリップされる
                "frame.fill_rect(2, 2, 10, 10, 255, 0, 0, 255)",
                &clip(),
                &no_params(),
                4,
                4,
                &mut pixels,
            )
            .unwrap();
        let px = |x: usize, y: usize| &pixels[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
        assert_eq!(px(0, 0), &[0, 0, 0, 0]);
        assert_eq!(px(2, 2), &[255, 0, 0, 255]);
        assert_eq!(px(3, 3), &[255, 0, 0, 255]);
        assert_eq!(px(1, 3), &[0, 0, 0, 0]);
    }

    #[test]
    fn frame_effect_uses_params_and_clip_time() {
        let engine = ScriptEngine::new();
        let mut c = clip();
        c.time_ms = 1500;
        let mut params = ScriptParams::new();
        params.insert("level".to_string(), ScriptParam::Number(7.0));
        let mut pixels = vec![0u8; 4];
        engine
            .run_frame_effect(
                "frame.set(0, 0, clip.time_ms / 100, params.level, 0, 255)",
                &c,
                &params,
                1,
                1,
                &mut pixels,
            )
            .unwrap();
        assert_eq!(pixels, vec![15, 7, 0, 255]);
    }

    #[test]
    fn frame_effect_error_is_structured() {
        let engine = ScriptEngine::new();
        let mut pixels = vec![0u8; 4];
        let err = engine
            .run_frame_effect("error('fx boom')", &clip(), &no_params(), 1, 1, &mut pixels)
            .unwrap_err();
        assert_eq!(err.kind, ScriptErrorKind::Runtime);
        assert!(err.message.contains("fx boom"), "{}", err.message);
    }

    #[test]
    fn frame_effect_out_of_bounds_is_error() {
        let engine = ScriptEngine::new();
        let mut pixels = vec![0u8; 4];
        let err = engine
            .run_frame_effect(
                "frame.set(3, 0, 1, 2, 3, 255)",
                &clip(),
                &no_params(),
                1,
                1,
                &mut pixels,
            )
            .unwrap_err();
        assert_eq!(err.kind, ScriptErrorKind::Runtime);
    }

    #[test]
    fn frame_effect_infinite_loop_is_interrupted() {
        let engine = ScriptEngine::with_instruction_limit(20_000);
        let mut pixels = vec![0u8; 4];
        let err = engine
            .run_frame_effect(
                "while true do end",
                &clip(),
                &no_params(),
                1,
                1,
                &mut pixels,
            )
            .unwrap_err();
        assert_eq!(err.kind, ScriptErrorKind::InstructionLimit);
    }

    // ---- 構文検証（Issue #82） ----

    #[test]
    fn check_syntax_accepts_valid_source() {
        let engine = ScriptEngine::new();
        assert_eq!(engine.check_syntax("return 1 + 2"), Ok(()));
    }

    #[test]
    fn check_syntax_rejects_broken_source_as_syntax() {
        let engine = ScriptEngine::new();
        let err = engine.check_syntax("return 1 +").unwrap_err();
        assert_eq!(err.kind, ScriptErrorKind::Syntax);
    }

    #[test]
    fn check_syntax_does_not_execute() {
        // コンパイルのみ: 実行時エラーや無限ループを含むソースでも
        // 構文が正しければ即座に Ok を返す。
        let engine = ScriptEngine::new();
        assert_eq!(engine.check_syntax("error('boom')"), Ok(()));
        assert_eq!(engine.check_syntax("while true do end"), Ok(()));
    }
}
