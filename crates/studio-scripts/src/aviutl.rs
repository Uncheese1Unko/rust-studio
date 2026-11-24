//! AviUtl 拡張編集（ExEdit）Lua スクリプト互換プロファイル（Issue #83）。
//!
//! ADR-0019（方式 A）に基づくクリーンルーム実装。対応範囲と根拠は
//! `docs/aviutl-compat.md` と #79 の調査レポート
//! （`docs/test-reports/issue-0079-aviutl-compat-investigation.md`）に置く。
//!
//! 設計の要点:
//! - `.anm` / `.obj` ファイルは SJIS バイト列のまま扱う。セクション
//!   分割とディレクティブ解釈はバイトレベルで行い（SJIS の第2バイトは
//!   ASCII 区切り文字と衝突しない）、UTF-8 への変換はホスト境界
//!   （セクション名・ラベル・描画文字列）でのみ行う。
//! - Lua 実行の土台（サンドボックス・命令数上限）は engine を共有する。
//!   ここでは `obj` グローバルとバッファモデルを足すだけに留める。
//! - 未対応の API・効果は、黙って壊さず明示的な実行時エラーにする
//!   （ADR-0019 の方針）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::engine::{
    classify_error, runtime_error, ClipInfo, ScriptEngine, ScriptError, ScriptParam, ScriptParams,
};

/// SJIS バイト列を文字列へ（変換できないバイトは代替文字）。
pub fn decode_sjis(bytes: &[u8]) -> String {
    encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned()
}

/// 文字列を SJIS バイト列へ（テスト用のサンプル生成にも使う）。
pub fn encode_sjis(text: &str) -> Vec<u8> {
    encoding_rs::SHIFT_JIS.encode(text).0.into_owned()
}

/// RGBA8 画像バッファ（幅・高さ・ピクセル列）。
#[derive(Debug, Clone, PartialEq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl RgbaImage {
    pub fn new(width: u32, height: u32, fill: [u8; 4]) -> Self {
        let mut data = vec![0u8; (width as usize) * (height as usize) * 4];
        for px in data.as_chunks_mut::<4>().0 {
            px.copy_from_slice(&fill);
        }
        Self {
            width,
            height,
            data,
        }
    }

    pub fn from_pixels(width: u32, height: u32, data: Vec<u8>) -> Self {
        debug_assert_eq!(data.len(), (width as usize) * (height as usize) * 4);
        Self {
            width,
            height,
            data,
        }
    }

    pub fn transparent(width: u32, height: u32) -> Self {
        Self::new(width, height, [0, 0, 0, 0])
    }

    pub fn get(&self, x: i64, y: i64) -> Option<[u8; 4]> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        let idx = ((y * self.width as i64 + x) * 4) as usize;
        Some([
            self.data[idx],
            self.data[idx + 1],
            self.data[idx + 2],
            self.data[idx + 3],
        ])
    }

    pub fn set(&mut self, x: i64, y: i64, rgba: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return;
        }
        let idx = ((y * self.width as i64 + x) * 4) as usize;
        self.data[idx..idx + 4].copy_from_slice(&rgba);
    }
}

/// `--trackN:` の定義（ラベル・最小・最大・初期値）。
#[derive(Debug, Clone, PartialEq)]
pub struct TrackDef {
    pub label: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
}

/// `--dialog:` の変数の種別（変数名の接尾辞で決まる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogKind {
    /// 汎用（数値か文字列。初期値の形で決まる）。
    Value,
    /// `/chk`: チェックボックス（真偽）。
    Check,
    /// `/col`: 色（数値 0xRRGGBB）。
    Color,
    /// `/fig`: 図形名（文字列）。
    Figure,
}

/// `--dialog:` で定義される名前付き変数。
#[derive(Debug, Clone, PartialEq)]
pub struct DialogVar {
    pub label: String,
    pub name: String,
    pub kind: DialogKind,
    pub default: ScriptParam,
}

/// `.anm` / `.obj` の1セクション（`@名前` から次の `@` まで）。
#[derive(Debug, Clone, PartialEq)]
pub struct AnmSection {
    /// セクション名（SJIS を復号したもの）。ファイル先頭の無名部は ""。
    pub name: String,
    pub tracks: [Option<TrackDef>; 4],
    /// `--check0:`（ラベルと初期値）。
    pub check0: Option<(String, bool)>,
    pub dialog: Vec<DialogVar>,
    /// `--color:` の初期値（変数 `color`）。
    pub color_default: Option<f64>,
    /// `--file:` の有無（変数 `file`）。
    pub has_file: bool,
    /// `--param:` の内容（本体の前に実行するコード片）。
    pub param_code: Vec<u8>,
    /// セクション本体（ディレクティブ行を含む生バイト列。
    /// ディレクティブは Lua コメントなので実行に影響しない）。
    pub body: Vec<u8>,
}

impl AnmSection {
    fn empty(name: String) -> Self {
        Self {
            name,
            tracks: [None, None, None, None],
            check0: None,
            dialog: Vec::new(),
            color_default: None,
            has_file: false,
            param_code: Vec::new(),
            body: Vec::new(),
        }
    }
}

/// `.anm` / `.obj` ファイル全体。
#[derive(Debug, Clone, PartialEq)]
pub struct AnmFile {
    pub sections: Vec<AnmSection>,
}

/// バイト列を行へ割る（CRLF / LF 両対応、末尾の \r は落とす）。
fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    bytes
        .split(|b| *b == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .collect()
}

/// 行頭の空白を落としたビュー。
fn trimmed(line: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < line.len() && (line[start] == b' ' || line[start] == b'\t') {
        start += 1;
    }
    &line[start..]
}

/// `--xxx:` ディレクティブなら接頭辞を落とした残りを返す。
fn directive<'a>(line: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let t = trimmed(line);
    let prefix = format!("--{name}:");
    t.strip_prefix(prefix.as_bytes())
}

/// 数値として読む（読めなければ既定値）。
fn parse_number(bytes: &[u8], default: f64) -> f64 {
    decode_sjis(bytes).trim().parse::<f64>().unwrap_or(default)
}

/// `0xRRGGBB` 形式の色を読む。
fn parse_color(bytes: &[u8]) -> Option<f64> {
    let text = decode_sjis(bytes);
    let text = text.trim();
    let hex = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))?;
    i64::from_str_radix(hex.trim(), 16).ok().map(|v| v as f64)
}

/// `--dialog:` の1項目（`表示名[/種類],変数名=初期値`）を解釈する。
/// lua.txt の実形式では、種類の接尾辞（/chk・/col・/fig）は
/// **表示名の最後**に付き、文字列の初期値は引用符で囲まれる
/// （レビュー round 2 Major で是正。旧実装は変数名側の接尾辞を
/// 読み、引用符を外していなかった）。
fn parse_dialog_item(item: &[u8]) -> Option<DialogVar> {
    let text = decode_sjis(item);
    let (raw_label, rest) = text.split_once(',')?;
    let (raw_name, default_text) = rest.split_once('=')?;
    let (label, kind) = if let Some(l) = raw_label.strip_suffix("/chk") {
        (l, DialogKind::Check)
    } else if let Some(l) = raw_label.strip_suffix("/col") {
        (l, DialogKind::Color)
    } else if let Some(l) = raw_label.strip_suffix("/fig") {
        (l, DialogKind::Figure)
    } else {
        (raw_label, DialogKind::Value)
    };
    let default_text = default_text.trim();
    // 引用符で囲まれた初期値は引用符を外して文字列として保持する。
    let unquoted = default_text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'));
    let default = match kind {
        DialogKind::Check => ScriptParam::Bool(default_text != "0" && !default_text.is_empty()),
        DialogKind::Color => {
            ScriptParam::Number(parse_color(default_text.as_bytes()).unwrap_or(0.0))
        }
        DialogKind::Figure => ScriptParam::Text(unquoted.unwrap_or(default_text).to_string()),
        DialogKind::Value => match unquoted {
            Some(s) => ScriptParam::Text(s.to_string()),
            None => match default_text.parse::<f64>() {
                Ok(n) => ScriptParam::Number(n),
                Err(_) => ScriptParam::Text(default_text.to_string()),
            },
        },
    };
    Some(DialogVar {
        label: label.trim().to_string(),
        name: raw_name.trim().to_string(),
        kind,
        default,
    })
}

/// `.anm` / `.obj` のバイト列をセクションとディレクティブへ解釈する。
/// 解釈は決して失敗しない（最悪でも無名セクション1つになる）。
pub fn parse_anm(bytes: &[u8]) -> AnmFile {
    let mut sections: Vec<AnmSection> = Vec::new();
    let mut current = AnmSection::empty(String::new());
    let mut has_content = false;
    for line in split_lines(bytes) {
        if let Some(rest) = line.strip_prefix(b"@") {
            if has_content || !sections.is_empty() {
                sections.push(std::mem::replace(
                    &mut current,
                    AnmSection::empty(decode_sjis(rest).trim().to_string()),
                ));
            } else {
                current.name = decode_sjis(rest).trim().to_string();
            }
            has_content = true;
            continue;
        }
        has_content = true;
        // ディレクティブ解釈（本体にも行は残す。Lua コメントのため）。
        for (i, track) in current.tracks.iter_mut().enumerate() {
            if let Some(rest) = directive(line, &format!("track{i}")) {
                let parts: Vec<&[u8]> = rest.split(|b| *b == b',').collect();
                if parts.len() >= 4 {
                    *track = Some(TrackDef {
                        label: decode_sjis(parts[0]).trim().to_string(),
                        min: parse_number(parts[1], 0.0),
                        max: parse_number(parts[2], 0.0),
                        default: parse_number(parts[3], 0.0),
                    });
                }
            }
        }
        if let Some(rest) = directive(line, "check0") {
            let parts: Vec<&[u8]> = rest.split(|b| *b == b',').collect();
            if parts.len() >= 2 {
                current.check0 = Some((
                    decode_sjis(parts[0]).trim().to_string(),
                    parse_number(parts[1], 0.0) != 0.0,
                ));
            }
        }
        if let Some(rest) = directive(line, "dialog") {
            for item in rest.split(|b| *b == b';') {
                if trimmed(item).is_empty() {
                    continue;
                }
                if let Some(var) = parse_dialog_item(item) {
                    current.dialog.push(var);
                }
            }
        }
        if let Some(rest) = directive(line, "color") {
            current.color_default = parse_color(rest);
        }
        if directive(line, "file").is_some() {
            current.has_file = true;
        }
        if let Some(rest) = directive(line, "param") {
            if !current.param_code.is_empty() {
                current.param_code.push(b'\n');
            }
            current.param_code.extend_from_slice(rest);
        }
        current.body.extend_from_slice(line);
        current.body.push(b'\n');
    }
    sections.push(current);
    AnmFile { sections }
}

/// 参照文字列（`ファイル.anm@セクション名`）をファイル部と
/// セクション名に分ける。拡張子の直後の `@` だけを区切りに使う
/// （配布ファイル名自体が `@` で始まることがあるため）。
pub fn split_reference(reference: &str) -> (&str, Option<&str>) {
    let lower = reference.to_lowercase();
    for ext in [".anm@", ".obj@"] {
        if let Some(pos) = lower.find(ext) {
            let cut = pos + ext.len();
            return (&reference[..cut - 1], Some(&reference[cut..]));
        }
    }
    (reference, None)
}

/// カタログ用: `.anm` / `.obj` の構文を検証する。問題があれば
/// その内容を返し、無ければ None を返す。各セクションの本体と
/// `--param:` の注入コードをコンパイルだけして確かめる。
pub fn check_anm_syntax(engine: &ScriptEngine, bytes: &[u8]) -> Option<String> {
    let file = parse_anm(bytes);
    for section in &file.sections {
        if !trimmed(&section.param_code).is_empty() {
            if let Err(e) = engine.check_syntax_bytes(&section.param_code) {
                return Some(e.message);
            }
        }
        if !trimmed(&section.body).is_empty() {
            if let Err(e) = engine.check_syntax_bytes(&section.body) {
                return Some(e.message);
            }
        }
    }
    None
}

/// セクションを選ぶ。名前指定があれば一致するものを、無ければ
/// 本体のある最初のセクション（無ければ先頭）を返す。
fn select_section<'a>(
    file: &'a AnmFile,
    name: Option<&str>,
) -> Result<&'a AnmSection, ScriptError> {
    if let Some(name) = name {
        return file
            .sections
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| {
                runtime_error(format!(
                    "セクション「{name}」が見つかりません（あるセクション: {}）",
                    file.sections
                        .iter()
                        .map(|s| if s.name.is_empty() {
                            "(無名)"
                        } else {
                            &s.name
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            });
    }
    Ok(file
        .sections
        .iter()
        .find(|s| !trimmed(&s.body).is_empty())
        .unwrap_or(&file.sections[0]))
}

/// パラメータを数値として引く（無ければ既定値）。
fn param_number(params: &ScriptParams, key: &str, default: f64) -> f64 {
    match params.get(key) {
        Some(ScriptParam::Number(n)) => *n,
        Some(ScriptParam::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(ScriptParam::Text(t)) => t.parse().unwrap_or(default),
        None => default,
    }
}

/// パラメータを真偽として引く（無ければ既定値）。
fn param_bool(params: &ScriptParams, key: &str, default: bool) -> bool {
    match params.get(key) {
        Some(ScriptParam::Bool(b)) => *b,
        Some(ScriptParam::Number(n)) => *n != 0.0,
        Some(ScriptParam::Text(t)) => t == "1" || t.eq_ignore_ascii_case("true"),
        None => default,
    }
}

/// パラメータを文字列として引く（無ければ既定値）。
fn param_text(params: &ScriptParams, key: &str, default: &str) -> String {
    match params.get(key) {
        Some(ScriptParam::Text(t)) => t.clone(),
        Some(ScriptParam::Number(n)) => n.to_string(),
        Some(ScriptParam::Bool(b)) => {
            if *b {
                "1".into()
            } else {
                "0".into()
            }
        }
        None => default.to_string(),
    }
}

// ---- 実行部: バッファモデルと描画プリミティブ ----

/// 合成モード（obj.draw / setoption("blend")）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Blend {
    Normal,
    Add,
    Sub,
    Mul,
    Screen,
    Overlay,
    Lighten,
    Darken,
    /// 輝度（8）: 色相・彩度は dst、明度は src（HSV での近似）。
    Luminosity,
    /// 色差（9）: 色相・彩度は src、明度は dst（HSV での近似）。
    Color,
}

impl Blend {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "通常" | "normal" => Some(Self::Normal),
            "加算" | "add" => Some(Self::Add),
            "減算" | "sub" => Some(Self::Sub),
            "乗算" | "mul" => Some(Self::Mul),
            "スクリーン" | "screen" => Some(Self::Screen),
            "オーバーレイ" | "overlay" => Some(Self::Overlay),
            "比較（明）" | "lighten" => Some(Self::Lighten),
            "比較（暗）" | "darken" => Some(Self::Darken),
            "輝度" => Some(Self::Luminosity),
            "色差" => Some(Self::Color),
            _ => None,
        }
    }

    /// lua.txt の数値指定: 0=通常 1=加算 2=減算 3=乗算
    /// 4=スクリーン 5=オーバーレイ 6=比較（明） 7=比較（暗）
    /// 8=輝度 9=色差（レビュー round 2 Major で是正。旧対応付けは
    /// 5/6 が比較（明）／比較（暗）にずれており、7〜9 はエラー
    /// だった）。
    fn from_index(index: i64) -> Option<Self> {
        match index {
            0 => Some(Self::Normal),
            1 => Some(Self::Add),
            2 => Some(Self::Sub),
            3 => Some(Self::Mul),
            4 => Some(Self::Screen),
            5 => Some(Self::Overlay),
            6 => Some(Self::Lighten),
            7 => Some(Self::Darken),
            8 => Some(Self::Luminosity),
            9 => Some(Self::Color),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Normal => "通常",
            Self::Add => "加算",
            Self::Sub => "減算",
            Self::Mul => "乗算",
            Self::Screen => "スクリーン",
            Self::Overlay => "オーバーレイ",
            Self::Lighten => "比較（明）",
            Self::Darken => "比較（暗）",
            Self::Luminosity => "輝度",
            Self::Color => "色差",
        }
    }

    fn channel(self, dst: f64, src: f64) -> f64 {
        match self {
            Self::Normal => src,
            Self::Add => (dst + src).min(255.0),
            Self::Sub => (dst - src).max(0.0),
            Self::Mul => dst * src / 255.0,
            Self::Screen => 255.0 - (255.0 - dst) * (255.0 - src) / 255.0,
            Self::Overlay => {
                if dst < 127.5 {
                    2.0 * dst * src / 255.0
                } else {
                    255.0 - 2.0 * (255.0 - dst) * (255.0 - src) / 255.0
                }
            }
            Self::Lighten => dst.max(src),
            Self::Darken => dst.min(src),
            // 輝度・色差はチャンネル独立ではないため blend_pixel 側
            // で画素単位に処理する。ここには来ない。
            Self::Luminosity | Self::Color => src,
        }
    }
}

/// 描画先（setoption("drawtarget")）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Frm,
    Obj,
    Tmp,
}

/// フォント設定（obj.setfont の保持分。グリフはホストの
/// システムフォントで描くため、フォント名は記録のみ）。
#[derive(Debug, Clone)]
struct FontSetting {
    size: f64,
    /// ExEdit の装飾タイプ（0: 通常、1: 影付き、2: 影付き（薄）、
    /// 3: 縁取り、4: 縁取り（細）、5: 縁取り（太）、6: 縁取り（角））。
    decoration: u32,
    /// 文字色（col1）。
    color: u32,
    /// 影・縁の色（col2）。
    color2: u32,
    bold: bool,
    italic: bool,
    spacing_x: f64,
    spacing_y: f64,
}

impl Default for FontSetting {
    fn default() -> Self {
        Self {
            size: 24.0,
            decoration: 0,
            color: 0xFFFFFF,
            color2: 0x000000,
            bold: false,
            italic: false,
            spacing_x: 0.0,
            spacing_y: 0.0,
        }
    }
}

/// 実行中のホスト状態。Lua クロージャからは Rc<RefCell<>> で共有する。
struct Host {
    canvas: RgbaImage,
    objbuf: RgbaImage,
    tempbuf: Option<RgbaImage>,
    cache: HashMap<String, RgbaImage>,
    source: RgbaImage,
    drawn: bool,
    drawtarget: Target,
    blend: Blend,
    /// pixeloption のピクセル形式（false=col / true=rgb）。
    pixel_rgb: bool,
    /// pixeloption("get",...) の読み取り先（false=obj / true=frm）。
    pixel_get_frm: bool,
    /// pixeloption("put",...) の書き込み先（false=obj / true=frm）。
    pixel_put_frm: bool,
    /// pixeloption("blend",...) の putpixel 合成モード
    /// （None = そのまま上書き）。
    pixel_blend: Option<Blend>,
    font: FontSetting,
    rng: u64,
    scripts_dir: PathBuf,
    section_name: String,
    obj_table: Option<mlua::Table>,
}

impl Host {
    fn obj_num(&self, key: &str, default: f64) -> f64 {
        match &self.obj_table {
            Some(table) => table.get::<f64>(key).unwrap_or(default),
            None => default,
        }
    }

    fn sync_obj_dims(&self) {
        if let Some(table) = &self.obj_table {
            let _ = table.set("w", self.objbuf.width);
            let _ = table.set("h", self.objbuf.height);
        }
    }

    fn next_rand(&mut self) -> u64 {
        // xorshift64*（決定的。テストで固定できる）。
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// 1ピクセルの合成（モード適用後に alpha で混合する）。
fn blend_pixel(dst: &mut [u8], src: [u8; 4], alpha_mul: f64, mode: Blend) {
    let a = (src[3] as f64 / 255.0 * alpha_mul).clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    // 輝度・色差はチャンネル独立に計算できないため、HSV を経由して
    // 画素単位で合成色を作る（近似。docs/aviutl-compat.md に明記）。
    let blended_rgb: [f64; 3] = match mode {
        Blend::Luminosity | Blend::Color => {
            let (dh, ds, dv) = rgb_to_hsv(
                dst[0] as f64 / 255.0,
                dst[1] as f64 / 255.0,
                dst[2] as f64 / 255.0,
            );
            let (sh, ss, sv) = rgb_to_hsv(
                src[0] as f64 / 255.0,
                src[1] as f64 / 255.0,
                src[2] as f64 / 255.0,
            );
            let (h, s, v) = if mode == Blend::Luminosity {
                (dh, ds, sv)
            } else {
                (sh, ss, dv)
            };
            let (r, g, b) = hsv_to_rgb(h, s, v);
            [r * 255.0, g * 255.0, b * 255.0]
        }
        _ => [
            mode.channel(dst[0] as f64, src[0] as f64),
            mode.channel(dst[1] as f64, src[1] as f64),
            mode.channel(dst[2] as f64, src[2] as f64),
        ],
    };
    for ch in 0..3 {
        dst[ch] = (blended_rgb[ch] * a + dst[ch] as f64 * (1.0 - a))
            .round()
            .clamp(0.0, 255.0) as u8;
    }
    let da = dst[3] as f64 / 255.0;
    dst[3] = ((a + da * (1.0 - a)) * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// src を dst へアフィン合成する。center は dst 座標での src 中心、
/// rot はラジアン、(sx, sy) は拡大率、(rcx, rcy) は回転中心の
/// src 中心からのずらし。サンプリングは最近傍。
#[allow(clippy::too_many_arguments)]
fn composite(
    dst: &mut RgbaImage,
    src: &RgbaImage,
    center: (f64, f64),
    rot: f64,
    sx: f64,
    sy: f64,
    rot_center: (f64, f64),
    alpha_mul: f64,
    mode: Blend,
) {
    if sx == 0.0 || sy == 0.0 {
        return;
    }
    let (cos, sin) = (rot.cos(), rot.sin());
    let half = (src.width as f64 / 2.0, src.height as f64 / 2.0);
    for dy in 0..dst.height as i64 {
        for dx in 0..dst.width as i64 {
            let px = dx as f64 + 0.5 - center.0;
            let py = dy as f64 + 0.5 - center.1;
            // 逆回転してから拡大率で割る。
            let qx = (px * cos + py * sin) / sx;
            let qy = (-px * sin + py * cos) / sy;
            let sxi = (qx + half.0 + rot_center.0).floor() as i64;
            let syi = (qy + half.1 + rot_center.1).floor() as i64;
            if let Some(px_src) = src.get(sxi, syi) {
                let idx = ((dy * dst.width as i64 + dx) * 4) as usize;
                let mut cell = [
                    dst.data[idx],
                    dst.data[idx + 1],
                    dst.data[idx + 2],
                    dst.data[idx + 3],
                ];
                blend_pixel(&mut cell, px_src, alpha_mul, mode);
                dst.data[idx..idx + 4].copy_from_slice(&cell);
            }
        }
    }
}

/// src を dst へそのまま（変形なしで）重ねる。最終フォールバック用。
fn blit_over(dst: &mut RgbaImage, src: &RgbaImage) {
    let w = dst.width.min(src.width);
    let h = dst.height.min(src.height);
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if let Some(px_src) = src.get(x, y) {
                let idx = ((y * dst.width as i64 + x) * 4) as usize;
                let mut cell = [
                    dst.data[idx],
                    dst.data[idx + 1],
                    dst.data[idx + 2],
                    dst.data[idx + 3],
                ];
                blend_pixel(&mut cell, px_src, 1.0, Blend::Normal);
                dst.data[idx..idx + 4].copy_from_slice(&cell);
            }
        }
    }
}

/// 8x8 連立方程式を解く（射影変換の係数決定用）。
fn solve8(mut a: [[f64; 8]; 8], mut b: [f64; 8]) -> Option<[f64; 8]> {
    for col in 0..8 {
        let pivot = (col..8).max_by(|&r1, &r2| {
            a[r1][col]
                .abs()
                .partial_cmp(&a[r2][col].abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
        if a[pivot][col].abs() < 1e-12 {
            return None;
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        let div = a[col][col];
        for v in a[col].iter_mut().skip(col) {
            *v /= div;
        }
        b[col] /= div;
        for row in 0..8 {
            if row != col && a[row][col] != 0.0 {
                let factor = a[row][col];
                for j in col..8 {
                    a[row][j] -= factor * a[col][j];
                }
                b[row] -= factor * b[col];
            }
        }
    }
    Some(b)
}

/// src の四隅（src_quad、src ピクセル座標）を quad（dst 座標、
/// 左上・右上・右下・左下）へ写す射影変換で合成する
/// （obj.drawpoly）。src_quad が UV 指定にあたる。
fn composite_poly(
    dst: &mut RgbaImage,
    src: &RgbaImage,
    src_quad: [(f64, f64); 4],
    quad: [(f64, f64); 4],
    alpha_mul: f64,
    mode: Blend,
) {
    let corners = src_quad;
    // dst = H(src): X = (a x + b y + c) / (g x + h y + 1) 等の8係数。
    let mut a = [[0.0; 8]; 8];
    let mut b = [0.0; 8];
    for i in 0..4 {
        let (x, y) = corners[i];
        let (dx, dy) = quad[i];
        a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -dx * x, -dx * y];
        b[2 * i] = dx;
        a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -dy * x, -dy * y];
        b[2 * i + 1] = dy;
    }
    let Some(c) = solve8(a, b) else {
        return;
    };
    // 逆写像: dst ピクセルから src 座標へ（3x3 行列の逆行列）。
    let m = [[c[0], c[1], c[2]], [c[3], c[4], c[5]], [c[6], c[7], 1.0]];
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return;
    }
    let inv = [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) / det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) / det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) / det,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) / det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) / det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) / det,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) / det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) / det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) / det,
        ],
    ];
    for dy in 0..dst.height as i64 {
        for dx in 0..dst.width as i64 {
            let px = dx as f64 + 0.5;
            let py = dy as f64 + 0.5;
            let sw = inv[2][0] * px + inv[2][1] * py + inv[2][2];
            if sw == 0.0 {
                continue;
            }
            let sxi = ((inv[0][0] * px + inv[0][1] * py + inv[0][2]) / sw).floor() as i64;
            let syi = ((inv[1][0] * px + inv[1][1] * py + inv[1][2]) / sw).floor() as i64;
            if let Some(px_src) = src.get(sxi, syi) {
                let idx = ((dy * dst.width as i64 + dx) * 4) as usize;
                let mut cell = [
                    dst.data[idx],
                    dst.data[idx + 1],
                    dst.data[idx + 2],
                    dst.data[idx + 3],
                ];
                blend_pixel(&mut cell, px_src, alpha_mul, mode);
                dst.data[idx..idx + 4].copy_from_slice(&cell);
            }
        }
    }
}

/// 点が多角形の内側か（レイキャスティング）。
fn point_in_polygon(x: f64, y: f64, poly: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 図形をラスタライズする（obj.load("figure", ...)）。
/// 色は 0xRRGGBB、大きさは一辺（円は直径）。
fn figure_image(name: &str, color: u32, size: f64, line: f64) -> Result<RgbaImage, String> {
    let s = size.round().clamp(1.0, 2048.0) as u32;
    let rgba = [
        ((color >> 16) & 0xFF) as u8,
        ((color >> 8) & 0xFF) as u8,
        (color & 0xFF) as u8,
        255,
    ];
    let mut img = RgbaImage::transparent(s, s);
    let sf = s as f64;
    let regular = |n: usize, radius: f64| -> Vec<(f64, f64)> {
        (0..n)
            .map(|i| {
                let ang =
                    -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::TAU / n as f64;
                (sf / 2.0 + radius * ang.cos(), sf / 2.0 + radius * ang.sin())
            })
            .collect()
    };
    let polygon: Option<Vec<(f64, f64)>> = match name {
        "三角形" => Some(vec![
            (sf / 2.0, sf * 0.05),
            (sf * 0.05, sf * 0.95),
            (sf * 0.95, sf * 0.95),
        ]),
        "五角形" => Some(regular(5, sf / 2.0)),
        "六角形" => Some(regular(6, sf / 2.0)),
        "星形" => {
            let mut pts = Vec::new();
            for i in 0..10 {
                let ang = -std::f64::consts::FRAC_PI_2 + i as f64 * std::f64::consts::PI / 5.0;
                let r = if i % 2 == 0 {
                    sf / 2.0
                } else {
                    sf / 2.0 * 0.382
                };
                pts.push((sf / 2.0 + r * ang.cos(), sf / 2.0 + r * ang.sin()));
            }
            Some(pts)
        }
        _ => None,
    };
    let mut mask = vec![false; (s * s) as usize];
    for y in 0..s as i64 {
        for x in 0..s as i64 {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let inside = match name {
                "四角形" => true,
                "円" => {
                    let d = (px - sf / 2.0).powi(2) + (py - sf / 2.0).powi(2);
                    d <= (sf / 2.0).powi(2)
                }
                _ => match &polygon {
                    Some(poly) => point_in_polygon(px, py, poly),
                    None => {
                        return Err(format!(
                            "未対応の図形です: {name}（対応: 四角形・円・三角形・五角形・六角形・星形）"
                        ))
                    }
                },
            };
            if inside {
                mask[(y * s as i64 + x) as usize] = true;
            }
        }
    }
    // line（ライン幅）指定時は輪郭だけを残す: マスク内部の画素の
    // うち、周囲 line 以内にマスク外（または画像外）があるもの
    // だけを描く（レビュー round 2 Minor で実装）。
    let lw = line.round() as i64;
    for y in 0..s as i64 {
        for x in 0..s as i64 {
            if !mask[(y * s as i64 + x) as usize] {
                continue;
            }
            let keep = if lw <= 0 {
                true
            } else {
                let mut outline = false;
                'scan: for dy in -lw..=lw {
                    for dx in -lw..=lw {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx < 0
                            || ny < 0
                            || nx >= s as i64
                            || ny >= s as i64
                            || !mask[(ny * s as i64 + nx) as usize]
                        {
                            outline = true;
                            break 'scan;
                        }
                    }
                }
                outline
            };
            if keep {
                img.set(x, y, rgba);
            }
        }
    }
    Ok(img)
}

/// RGB (0..1) → HSV (h: 0..360, s/v: 0..1)。
fn rgb_to_hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

/// HSV → RGB (0..1)。
fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h {
        hh if hh < 1.0 => (c, x, 0.0),
        hh if hh < 2.0 => (x, c, 0.0),
        hh if hh < 3.0 => (0.0, c, x),
        hh if hh < 4.0 => (0.0, x, c),
        hh if hh < 5.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (r + m, g + m, b + m)
}

/// 0xRRGGBB を作る（RGB() グローバル用）。
fn pack_color(r: f64, g: f64, b: f64) -> i64 {
    let ch = |v: f64| v.round().clamp(0.0, 255.0) as i64;
    (ch(r) << 16) | (ch(g) << 8) | ch(b)
}

/// obj.effect の初期セット（ADR-0021 で決定:
/// 色調補正・単色化・ぼかし・色反転）。objbuf へ適用する。
/// 未知の効果名は明示エラー、未知のパラメータは無視する。
fn apply_effect(img: &mut RgbaImage, name: &str, params: &[(String, f64)]) -> Result<(), String> {
    let get = |key: &str, default: f64| {
        params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| *v)
            .unwrap_or(default)
    };
    match name {
        "色調補正" => {
            let brightness = get("明るさ", 100.0) / 100.0;
            let saturation = get("彩度", 100.0) / 100.0;
            let hue = get("色相", 0.0);
            let contrast = get("コントラスト", 100.0) / 100.0;
            for px in img.data.as_chunks_mut::<4>().0 {
                let (h, s, v) = rgb_to_hsv(
                    px[0] as f64 / 255.0,
                    px[1] as f64 / 255.0,
                    px[2] as f64 / 255.0,
                );
                let (r, g, b) = hsv_to_rgb(
                    h + hue,
                    (s * saturation).clamp(0.0, 1.0),
                    (v * brightness).clamp(0.0, 1.0),
                );
                px[0] = (((r - 0.5) * contrast + 0.5) * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                px[1] = (((g - 0.5) * contrast + 0.5) * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                px[2] = (((b - 0.5) * contrast + 0.5) * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
            Ok(())
        }
        "単色化" => {
            let color = get("色", 0xFFFFFF as f64) as u32;
            let strength = (get("光度", 100.0) / 100.0).clamp(0.0, 1.0);
            let (cr, cg, cb) = (
                ((color >> 16) & 0xFF) as f64,
                ((color >> 8) & 0xFF) as f64,
                (color & 0xFF) as f64,
            );
            for px in img.data.as_chunks_mut::<4>().0 {
                let lum =
                    (0.299 * px[0] as f64 + 0.587 * px[1] as f64 + 0.114 * px[2] as f64) / 255.0;
                let target = [cr * lum, cg * lum, cb * lum];
                for ch in 0..3 {
                    px[ch] = (target[ch] * strength + px[ch] as f64 * (1.0 - strength))
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            }
            Ok(())
        }
        "ぼかし" => {
            let radius = get("範囲", 5.0).round().clamp(0.0, 32.0) as i64;
            if radius > 0 {
                box_blur(img, radius);
            }
            Ok(())
        }
        "色反転" => {
            for px in img.data.as_chunks_mut::<4>().0 {
                px[0] = 255 - px[0];
                px[1] = 255 - px[1];
                px[2] = 255 - px[2];
            }
            Ok(())
        }
        _ => Err(format!(
            "未対応のエフェクトです: {name}（対応: 色調補正・単色化・ぼかし・色反転）"
        )),
    }
}

/// 単純なボックスぼかし（水平→垂直の2パス、αも含む）。
fn box_blur(img: &mut RgbaImage, radius: i64) {
    let (w, h) = (img.width as i64, img.height as i64);
    let pass = |horizontal: bool, img: &mut RgbaImage| {
        let src = img.data.clone();
        let len = (2 * radius + 1) as f64;
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f64; 4];
                for k in -radius..=radius {
                    let (sx, sy) = if horizontal {
                        ((x + k).clamp(0, w - 1), y)
                    } else {
                        (x, (y + k).clamp(0, h - 1))
                    };
                    let idx = ((sy * w + sx) * 4) as usize;
                    for ch in 0..4 {
                        acc[ch] += src[idx + ch] as f64;
                    }
                }
                let idx = ((y * w + x) * 4) as usize;
                for ch in 0..4 {
                    img.data[idx + ch] = (acc[ch] / len).round() as u8;
                }
            }
        }
    };
    pass(true, img);
    pass(false, img);
}

/// システムフォントを1つ読み込む（CJK 優先、無ければ DejaVu）。
/// 見つからなければ None（文字描画は明示エラーになる）。
fn system_font() -> Option<&'static ab_glyph::FontVec> {
    static FONT: std::sync::OnceLock<Option<ab_glyph::FontVec>> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        let candidates = [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ];
        for path in candidates {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(font) = ab_glyph::FontVec::try_from_vec_and_index(bytes, 0) {
                    return Some(font);
                }
            }
        }
        None
    })
    .as_ref()
}

/// 文字列をフォント設定で描いた画像を作る（obj.load("text") /
/// obj.mes）。太字は二重描きで近似し、斜体は未対応（無視する。
/// docs/aviutl-compat.md に明記）。装飾タイプ（type）は、影付きを
/// col2 でのオフセット描画、縁取りを col2 での周囲描画で近似する。
fn text_image(text: &str, setting: &FontSetting) -> Result<RgbaImage, String> {
    use ab_glyph::{Font as _, PxScale, ScaleFont as _};
    let font =
        system_font().ok_or_else(|| "テキスト描画用のフォントが見つかりません".to_string())?;
    if text.is_empty() {
        return Ok(RgbaImage::transparent(1, 1));
    }
    let size = if setting.size > 0.0 {
        setting.size as f32
    } else {
        24.0
    };
    let scaled = font.as_scaled(PxScale::from(size));
    let line_height = (scaled.ascent() - scaled.descent()).ceil() as u32;
    let lines: Vec<&str> = text.split('\n').collect();
    let mut width = 0f32;
    for line in &lines {
        let mut w = 0f32;
        for ch in line.chars() {
            w += scaled.h_advance(font.glyph_id(ch)) + setting.spacing_x as f32;
        }
        width = width.max(w);
    }
    // 装飾の広がり分だけキャンバスに余白を取る（影は右下、
    // 縁取りは全周）。0 のときは余白なし。
    let pad: i64 = match setting.decoration {
        1 | 5 => 2,
        2 | 3 | 4 | 6 => 1,
        _ => 0,
    };
    let width = (width.ceil() as u32).max(1) + (pad as u32) * 2;
    let height = (line_height * lines.len() as u32
        + (setting.spacing_y as u32) * lines.len().saturating_sub(1) as u32)
        .max(1)
        + (pad as u32) * 2;
    let mut img = RgbaImage::transparent(width, height);
    let rgb_of = |color: u32| -> (u8, u8, u8) {
        (
            ((color >> 16) & 0xFF) as u8,
            ((color >> 8) & 0xFF) as u8,
            (color & 0xFF) as u8,
        )
    };
    let (cr, cg, cb) = rgb_of(setting.color);
    let (sr, sg, sb) = rgb_of(setting.color2);
    for (li, line) in lines.iter().enumerate() {
        let baseline =
            scaled.ascent() + li as f32 * (line_height as f32 + setting.spacing_y as f32);
        let mut pen_x = 0f32;
        for ch in line.chars() {
            let mut glyph = scaled.scaled_glyph(ch);
            glyph.position = ab_glyph::point(pen_x + pad as f32, baseline + pad as f32);
            if let Some(outline) = scaled.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                let draw_once = |img: &mut RgbaImage,
                                 offset_x: i64,
                                 offset_y: i64,
                                 (pr, pg, pb): (u8, u8, u8)| {
                    outline.draw(|gx, gy, coverage| {
                        if coverage <= 0.0 {
                            return;
                        }
                        let x = bounds.min.x as i64 + gx as i64 + offset_x;
                        let y = bounds.min.y as i64 + gy as i64 + offset_y;
                        let a = coverage.clamp(0.0, 1.0);
                        if let Some(dst) = img.get(x, y) {
                            let na = a + (dst[3] as f32 / 255.0) * (1.0 - a);
                            let mix = |d: u8, s: u8| -> u8 {
                                ((s as f32 * a + d as f32 * (1.0 - a)).round()).clamp(0.0, 255.0)
                                    as u8
                            };
                            img.set(
                                x,
                                y,
                                [
                                    mix(dst[0], pr),
                                    mix(dst[1], pg),
                                    mix(dst[2], pb),
                                    (na * 255.0).round() as u8,
                                ],
                            );
                        }
                    });
                };
                match setting.decoration {
                    // 影付き: col2 を右下へずらして先に描く。
                    1 | 2 => draw_once(&mut img, pad, pad, (sr, sg, sb)),
                    // 縁取り: col2 を周囲に描く。
                    3..=6 => {
                        for dy in -pad..=pad {
                            for dx in -pad..=pad {
                                if dx != 0 || dy != 0 {
                                    draw_once(&mut img, dx, dy, (sr, sg, sb));
                                }
                            }
                        }
                    }
                    _ => {}
                }
                draw_once(&mut img, 0, 0, (cr, cg, cb));
                if setting.bold {
                    draw_once(&mut img, 1, 0, (cr, cg, cb));
                }
            }
            pen_x += scaled.h_advance(font.glyph_id(ch)) + setting.spacing_x as f32;
        }
    }
    Ok(img)
}

// ---- 実行部: obj API と実行エントリ ----

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{MultiValue, String as LuaString, Value as LuaValue};

/// 未対応を明示する実行時エラーを作る。
fn unsupported(what: &str) -> mlua::Error {
    mlua::Error::runtime(format!("未対応: {what}"))
}

/// Lua 文字列引数を SJIS として復号する（名前・ラベル用）。
fn arg_text(value: &LuaValue) -> Option<String> {
    match value {
        LuaValue::String(s) => Some(decode_sjis(&s.as_bytes())),
        _ => None,
    }
}

/// `.anm` / `.obj` スクリプトをフレームへ適用する（Issue #83）。
///
/// - `section_name`: 参照に `@名前` があればそのセクション。
/// - `generated_object`: `.obj` のとき true（オブジェクトバッファは
///   透明から始まる）。`.anm` は入力フレームのコピーから始まる。
/// - `file_name`: 呼び出し側が参照を解決したファイル名。現在は
///   実行には使わない（`obj.getinfo("script_path")` は lua.txt
///   どおりフォルダのパスを返すため）が、API の対称性と将来の
///   エラー文脈用に受け取る。
/// - 描画規則: `obj.draw()` 等で画面（canvas）へ描かれたらその結果、
///   一度も描かれなければオブジェクトバッファをそのまま重ねた結果
///   （ピクセル操作だけのスクリプトがそのまま効くための規則。
///   docs/aviutl-compat.md に明記）。
#[allow(clippy::too_many_arguments)]
pub fn run_anm_effect(
    engine: &ScriptEngine,
    source_bytes: &[u8],
    section_name: Option<&str>,
    generated_object: bool,
    clip: &ClipInfo,
    params: &ScriptParams,
    fps: u32,
    width: u32,
    height: u32,
    pixels: &mut [u8],
    scripts_dir: &Path,
    _file_name: &str,
) -> Result<(), ScriptError> {
    let expected = (width as usize) * (height as usize) * 4;
    if pixels.len() != expected {
        return Err(runtime_error(format!(
            "フレームバッファの長さが寸法と一致しません: {width}x{height} には {expected} バイト必要ですが {} バイトです",
            pixels.len()
        )));
    }
    let file = parse_anm(source_bytes);
    let section = select_section(&file, section_name)?;

    // ディレクティブ由来の実効値（パラメータで上書きできる）。
    let mut track_values = [0.0f64; 4];
    for (i, def) in section.tracks.iter().enumerate() {
        let default = def.as_ref().map(|d| d.default).unwrap_or(0.0);
        track_values[i] = param_number(params, &format!("track{i}"), default);
    }
    let check0_value = {
        let default = section.check0.as_ref().map(|(_, d)| *d).unwrap_or(false);
        param_bool(params, "check0", default)
    };
    let dialog_values: Vec<(String, ScriptParam)> = section
        .dialog
        .iter()
        .map(|var| {
            let value = match (&var.default, params.get(&var.name)) {
                (_, Some(override_value)) => override_value.clone(),
                (ScriptParam::Number(d), None) => {
                    ScriptParam::Number(param_number(params, &var.name, *d))
                }
                (ScriptParam::Bool(d), None) => {
                    ScriptParam::Bool(param_bool(params, &var.name, *d))
                }
                (ScriptParam::Text(d), None) => ScriptParam::Text(param_text(params, &var.name, d)),
            };
            (var.name.clone(), value)
        })
        .collect();
    let color_value = match section.color_default {
        Some(d) => param_number(params, "color", d),
        None => param_number(params, "color", 0xFFFFFF as f64),
    };
    let file_value = param_text(params, "file", "");

    let source_image = RgbaImage::from_pixels(width, height, pixels.to_vec());
    let objbuf = if generated_object {
        RgbaImage::transparent(width, height)
    } else {
        source_image.clone()
    };
    let host = Rc::new(RefCell::new(Host {
        canvas: source_image.clone(),
        objbuf,
        tempbuf: None,
        cache: HashMap::new(),
        source: source_image,
        drawn: false,
        drawtarget: Target::Frm,
        blend: Blend::Normal,
        pixel_rgb: false,
        pixel_get_frm: false,
        pixel_put_frm: false,
        pixel_blend: None,
        font: FontSetting::default(),
        rng: 0x853c_49e6_748f_ea9b,
        scripts_dir: scripts_dir.to_path_buf(),
        section_name: section.name.clone(),
        obj_table: None,
    }));

    let (lua, exceeded) = engine.build_lua(clip, params)?;
    let section_body = section.body.clone();
    let param_code = section.param_code.clone();
    let result = lua.scope(|scope| -> mlua::Result<()> {
        let globals = lua.globals();

        // --- obj テーブル（状態変数） ---
        let obj = lua.create_table()?;
        for key in ["ox", "oy", "oz", "rx", "ry", "rz", "cx", "cy", "cz"] {
            obj.set(key, 0.0)?;
        }
        obj.set("zoom", 1.0)?;
        obj.set("alpha", 1.0)?;
        obj.set("aspect", 0.0)?;
        for (i, value) in track_values.iter().enumerate() {
            obj.set(format!("track{i}"), *value)?;
        }
        obj.set("check0", check0_value)?;
        obj.set("screen_w", width)?;
        obj.set("screen_h", height)?;
        obj.set("time", clip.time_ms as f64 / 1000.0)?;
        obj.set("totaltime", clip.duration_ms as f64 / 1000.0)?;
        let fps = fps.max(1) as f64;
        obj.set("frame", (clip.time_ms as f64 * fps / 1000.0).floor())?;
        obj.set(
            "totalframe",
            (clip.duration_ms as f64 * fps / 1000.0).floor(),
        )?;
        obj.set("x", 0.0)?;
        obj.set("y", 0.0)?;
        obj.set("z", 0.0)?;
        host.borrow_mut().obj_table = Some(obj.clone());
        host.borrow().sync_obj_dims();

        // --- ヘルパー: 描画先バッファへの合成 ---
        // （クロージャごとに host を掴むため、ここでは関数化せず
        //   各 obj.* 内で同じ手順を踏む）

        // obj.load(kind, ...)
        {
            let host = host.clone();
            let load = scope.create_function(move |lua, args: MultiValue| {
                let mut it = args.into_iter();
                let kind = match it.next() {
                    Some(v) => arg_text(&v).unwrap_or_default(),
                    None => return Err(mlua::Error::runtime("obj.load: 種別が必要です")),
                };
                let rest: Vec<LuaValue> = it.collect();
                let mut h = host.borrow_mut();
                match kind.as_str() {
                    "figure" => {
                        let name = rest.first().and_then(arg_text).ok_or_else(|| {
                            mlua::Error::runtime("obj.load(\"figure\"): 図形名が必要です")
                        })?;
                        let color = match rest.get(1) {
                            Some(LuaValue::Number(n)) => *n as u32,
                            Some(LuaValue::Integer(i)) => *i as u32,
                            _ => 0xFFFFFF,
                        };
                        let size = match rest.get(2) {
                            Some(LuaValue::Number(n)) => *n,
                            Some(LuaValue::Integer(i)) => *i as f64,
                            _ => 100.0,
                        };
                        let line = match rest.get(3) {
                            Some(LuaValue::Number(n)) => *n,
                            Some(LuaValue::Integer(i)) => *i as f64,
                            _ => 0.0,
                        };
                        h.objbuf = figure_image(&name, color, size, line)
                            .map_err(mlua::Error::runtime)?;
                    }
                    "text" => {
                        let text = rest.first().and_then(arg_text).ok_or_else(|| {
                            mlua::Error::runtime("obj.load(\"text\"): 文字列が必要です")
                        })?;
                        // lua.txt: obj.load("text",text[,speed,time])。
                        // speed は表示速度（文字/秒）で色ではない
                        // （レビュー round 2 Major で是正。旧実装は
                        // 第3引数を色として扱っていた）。色は
                        // setfont の col1 が決める。speed 指定時は
                        // 経過時間（省略時はオブジェクト時間）までに
                        // 表示される文字数だけに切り取る。
                        let setting = h.font.clone();
                        let text = match rest.get(1) {
                            Some(v) => {
                                let speed = match v {
                                    LuaValue::Number(n) => *n,
                                    LuaValue::Integer(i) => *i as f64,
                                    _ => 0.0,
                                };
                                let time = match rest.get(2) {
                                    Some(LuaValue::Number(n)) => *n,
                                    Some(LuaValue::Integer(i)) => *i as f64,
                                    _ => h.obj_num("time", 0.0),
                                };
                                if speed > 0.0 {
                                    let count =
                                        (speed * time).floor().max(0.0) as usize;
                                    text.chars().take(count).collect::<String>()
                                } else {
                                    text
                                }
                            }
                            None => text,
                        };
                        h.objbuf = text_image(&text, &setting).map_err(mlua::Error::runtime)?;
                    }
                    "image" => {
                        let path = match rest.first() {
                            Some(LuaValue::String(s)) => s.to_string_lossy(),
                            _ => {
                                return Err(mlua::Error::runtime(
                                    "obj.load(\"image\"): パスが必要です",
                                ))
                            }
                        };
                        let full = resolve_inside_scripts_dir(&h.scripts_dir, &path)
                            .map_err(mlua::Error::runtime)?;
                        let img = image::open(&full).map_err(|e| {
                            mlua::Error::runtime(format!(
                                "obj.load(\"image\"): {} を読めません: {e}",
                                full.display()
                            ))
                        })?;
                        let rgba = img.to_rgba8();
                        let (w, hh) = rgba.dimensions();
                        h.objbuf = RgbaImage::from_pixels(w, hh, rgba.into_raw());
                    }
                    "object" | "before" => {
                        h.objbuf = h.source.clone();
                    }
                    "framebuffer" => {
                        h.objbuf = h.canvas.clone();
                    }
                    "tempbuffer" => {
                        h.objbuf = h
                            .tempbuf
                            .clone()
                            .unwrap_or_else(|| RgbaImage::transparent(width, height));
                    }
                    "movie" => {
                        return Err(unsupported("obj.load(\"movie\")（動画ファイルの読み込み）"))
                    }
                    "layer" => return Err(unsupported("obj.load(\"layer\")（他レイヤーの取得）")),
                    other => {
                        return Err(mlua::Error::runtime(format!(
                            "obj.load: 未知の種別です: {other}"
                        )))
                    }
                }
                drop(h);
                let _ = lua;
                host.borrow().sync_obj_dims();
                Ok(())
            })?;
            obj.set("load", load)?;
        }

        // obj.draw([ox, oy, oz, zoom, alpha, rx, ry, rz])
        {
            let host = host.clone();
            let draw = scope.create_function(move |_, args: MultiValue| {
                let values: Vec<LuaValue> = args.into_iter().collect();
                let num = |i: usize| -> Option<f64> {
                    match values.get(i) {
                        Some(LuaValue::Number(n)) => Some(*n),
                        Some(LuaValue::Integer(n)) => Some(*n as f64),
                        _ => None,
                    }
                };
                let mut h = host.borrow_mut();
                // lua.txt の署名どおり。合成モード引数は無く、
                // blend は setoption の状態を使う（レビュー
                // round 2 Major で是正。旧対応付けは oz を zoom、
                // zoom を alpha と読み違えていた）。
                let ox = num(0).unwrap_or_else(|| h.obj_num("ox", 0.0));
                let oy = num(1).unwrap_or_else(|| h.obj_num("oy", 0.0));
                // num(2) は oz（奥行き）。2D 合成では使わない。
                let zoom = num(3).unwrap_or_else(|| h.obj_num("zoom", 1.0));
                let alpha = num(4).unwrap_or_else(|| h.obj_num("alpha", 1.0));
                let blend = h.blend;
                let aspect = h.obj_num("aspect", 0.0);
                let (sx, sy) = if aspect >= 0.0 {
                    (zoom * (1.0 - aspect), zoom)
                } else {
                    (zoom, zoom * (1.0 + aspect))
                };
                // num(5)/num(6) は rx/ry（3D 回転）で 2D では表現
                // できないため受け取るだけ（compat doc に明記）。
                let rot = num(7).unwrap_or_else(|| h.obj_num("rz", 0.0)).to_radians();
                let rc = (h.obj_num("cx", 0.0), h.obj_num("cy", 0.0));
                let src = h.objbuf.clone();
                match h.drawtarget {
                    Target::Frm => {
                        let center = (
                            h.canvas.width as f64 / 2.0 + ox,
                            h.canvas.height as f64 / 2.0 + oy,
                        );
                        composite(&mut h.canvas, &src, center, rot, sx, sy, rc, alpha, blend);
                        h.drawn = true;
                    }
                    Target::Obj => {
                        let center = (src.width as f64 / 2.0 + ox, src.height as f64 / 2.0 + oy);
                        let mut dst = h.objbuf.clone();
                        composite(&mut dst, &src, center, rot, sx, sy, rc, alpha, blend);
                        h.objbuf = dst;
                    }
                    Target::Tmp => {
                        let mut dst = h
                            .tempbuf
                            .clone()
                            .unwrap_or_else(|| RgbaImage::transparent(width, height));
                        let center = (dst.width as f64 / 2.0 + ox, dst.height as f64 / 2.0 + oy);
                        composite(&mut dst, &src, center, rot, sx, sy, rc, alpha, blend);
                        h.tempbuf = Some(dst);
                    }
                }
                Ok(())
            })?;
            obj.set("draw", draw)?;
        }

        // obj.drawpoly(x0,y0,z0, ..., x3,y3,z3 [, u0,v0, ..., u3,v3, alpha])
        {
            let host = host.clone();
            let drawpoly = scope.create_function(move |_, args: MultiValue| {
                let values: Vec<LuaValue> = args.into_iter().collect();
                let num = |i: usize| -> mlua::Result<f64> {
                    match values.get(i) {
                        Some(LuaValue::Number(n)) => Ok(*n),
                        Some(LuaValue::Integer(n)) => Ok(*n as f64),
                        _ => Err(mlua::Error::runtime(
                            "obj.drawpoly: 座標（数値×12）が必要です",
                        )),
                    }
                };
                let opt = |i: usize| -> Option<f64> {
                    match values.get(i) {
                        Some(LuaValue::Number(n)) => Some(*n),
                        Some(LuaValue::Integer(n)) => Some(*n as f64),
                        _ => None,
                    }
                };
                let mut h = host.borrow_mut();
                // lua.txt の署名どおり: 頂点は (x,y,z)×4、続いて
                // UV (u,v)×4 と alpha が任意（レビュー round 2
                // Major で是正。旧実装は (x,y)×4 + alpha + blend
                // と読み違えていた）。z は 2D 合成では使わない。
                let alpha = opt(20).unwrap_or_else(|| h.obj_num("alpha", 1.0));
                let blend = h.blend;
                let (cw, ch) = (h.canvas.width as f64 / 2.0, h.canvas.height as f64 / 2.0);
                let quad = [
                    (cw + num(0)?, ch + num(1)?),
                    (cw + num(3)?, ch + num(4)?),
                    (cw + num(6)?, ch + num(7)?),
                    (cw + num(9)?, ch + num(10)?),
                ];
                let (sw, sh) = (h.objbuf.width as f64, h.objbuf.height as f64);
                let mut src_quad = [(0.0, 0.0), (sw, 0.0), (sw, sh), (0.0, sh)];
                for (i, corner) in src_quad.iter_mut().enumerate() {
                    if let (Some(u), Some(v)) = (opt(12 + i * 2), opt(13 + i * 2)) {
                        *corner = (u, v);
                    }
                }
                let src = h.objbuf.clone();
                match h.drawtarget {
                    Target::Frm => {
                        composite_poly(&mut h.canvas, &src, src_quad, quad, alpha, blend);
                        h.drawn = true;
                    }
                    Target::Obj => {
                        let mut dst = h.objbuf.clone();
                        composite_poly(&mut dst, &src, src_quad, quad, alpha, blend);
                        h.objbuf = dst;
                    }
                    Target::Tmp => {
                        let mut dst = h
                            .tempbuf
                            .clone()
                            .unwrap_or_else(|| RgbaImage::transparent(width, height));
                        composite_poly(&mut dst, &src, src_quad, quad, alpha, blend);
                        h.tempbuf = Some(dst);
                    }
                }
                Ok(())
            })?;
            obj.set("drawpoly", drawpoly)?;
        }

        // obj.effect(name, key, value, ...)
        {
            let host = host.clone();
            let effect = scope.create_function(move |_, args: MultiValue| {
                let values: Vec<LuaValue> = args.into_iter().collect();
                let name = match values.first() {
                    Some(v) => arg_text(v).ok_or_else(|| {
                        mlua::Error::runtime("obj.effect: 効果名（文字列）が必要です")
                    })?,
                    None => {
                        return Err(unsupported(
                            "obj.effect() の引数なし呼び出し（後続エフェクトの実行）",
                        ))
                    }
                };
                let mut pairs: Vec<(String, f64)> = Vec::new();
                let mut i = 1;
                while i + 1 < values.len() + 1 && i < values.len() {
                    let key = arg_text(&values[i]);
                    let val = match values.get(i + 1) {
                        Some(LuaValue::Number(n)) => Some(*n),
                        Some(LuaValue::Integer(n)) => Some(*n as f64),
                        _ => None,
                    };
                    match (key, val) {
                        (Some(k), Some(v)) => pairs.push((k, v)),
                        _ => break,
                    }
                    i += 2;
                }
                let mut h = host.borrow_mut();
                apply_effect(&mut h.objbuf, &name, &pairs).map_err(mlua::Error::runtime)?;
                Ok(())
            })?;
            obj.set("effect", effect)?;
        }

        // obj.filter: 段階2のため未対応（明示エラー）。
        {
            let filter = scope.create_function(|_, _: MultiValue| {
                Err::<(), _>(unsupported("obj.filter（フィルタ効果）"))
            })?;
            obj.set("filter", filter)?;
        }

        // obj.copybuffer(dst, src)
        {
            let host = host.clone();
            let copybuffer = scope.create_function(move |_, (dst, src): (String, String)| {
                let mut h = host.borrow_mut();
                let image = resolve_buffer(&h, &src, width, height)?;
                match dst.as_str() {
                    "obj" => {
                        h.objbuf = image;
                        drop(h);
                        host.borrow().sync_obj_dims();
                    }
                    "tmp" => {
                        h.tempbuf = Some(image);
                    }
                    "frm" => {
                        if image.width == h.canvas.width && image.height == h.canvas.height {
                            h.canvas = image;
                        } else {
                            blit_over(&mut h.canvas, &image);
                        }
                    }
                    _ if dst.starts_with("cache:") => {
                        h.cache.insert(dst["cache:".len()..].to_string(), image);
                    }
                    other => {
                        return Err(mlua::Error::runtime(format!(
                            "obj.copybuffer: 書き込み先として使えません: {other}"
                        )))
                    }
                }
                // lua.txt: 戻り値は true=成功 / false=失敗。
                Ok(true)
            })?;
            obj.set("copybuffer", copybuffer)?;
        }

        // obj.setoption(name, value)
        {
            let host = host.clone();
            let setoption =
                scope.create_function(move |_, args: MultiValue| {
                    let values: Vec<LuaValue> = args.into_iter().collect();
                    let name = match values.first() {
                        Some(LuaValue::String(s)) => s.to_string_lossy(),
                        _ => {
                            return Err(mlua::Error::runtime(
                                "obj.setoption: オプション名（文字列）が必要です",
                            ))
                        }
                    };
                    let value = values.get(1).cloned().unwrap_or(LuaValue::Nil);
                    let opt_num = |i: usize| -> Option<f64> {
                        match values.get(i) {
                            Some(LuaValue::Number(n)) => Some(*n),
                            Some(LuaValue::Integer(n)) => Some(*n as f64),
                            _ => None,
                        }
                    };
                    let mut h = host.borrow_mut();
                    match name.as_str() {
                        "blend" => {
                            h.blend = match &value {
                                LuaValue::String(s) => {
                                    Blend::from_name(&decode_sjis(&s.as_bytes()))
                                }
                                LuaValue::Integer(i) => Blend::from_index(*i),
                                LuaValue::Number(n) => Blend::from_index(*n as i64),
                                _ => None,
                            }
                            .ok_or_else(|| {
                                mlua::Error::runtime(
                                    "obj.setoption(\"blend\"): 未知の合成モードです",
                                )
                            })?;
                        }
                        "drawtarget" => {
                            let target = match &value {
                                LuaValue::String(s) => s.to_string_lossy(),
                                _ => {
                                    return Err(mlua::Error::runtime(
                                        "obj.setoption(\"drawtarget\"): 文字列が必要です",
                                    ))
                                }
                            };
                            // lua.txt の正式名は "tempbuffer" /
                            // "framebuffer"（レビュー round 2 Major
                            // で是正）。短縮名 tmp/frm/obj も受ける。
                            h.drawtarget = match target.as_ref() {
                                "obj" => Target::Obj,
                                "tmp" | "tempbuffer" => Target::Tmp,
                                "frm" | "framebuffer" => Target::Frm,
                                other => {
                                    return Err(mlua::Error::runtime(format!(
                                        "obj.setoption(\"drawtarget\"): 未知の描画先です: {other}"
                                    )))
                                }
                            };
                            // tempbuffer は w,h 指定で透明に初期化
                            // される（lua.txt）。
                            if h.drawtarget == Target::Tmp {
                                if let (Some(w), Some(hh)) = (opt_num(2), opt_num(3)) {
                                    h.tempbuf = Some(RgbaImage::transparent(
                                        (w.max(0.0) as u32).max(1),
                                        (hh.max(0.0) as u32).max(1),
                                    ));
                                }
                            }
                        }
                        // 描画品質の指定は受け入れるが、最近傍サンプリングの
                        // 本実装では効果を持たない（docs に明記）。
                        "antialias" | "culling" => {}
                        other => return Err(unsupported(&format!("obj.setoption(\"{other}\")"))),
                    }
                    Ok(())
                })?;
            obj.set("setoption", setoption)?;
        }

        // obj.getoption(name) / obj.getinfo(name)
        {
            let host = host.clone();
            let getoption = scope.create_function(move |lua, name: String| {
                let h = host.borrow();
                match name.as_str() {
                    "script_name" => Ok(LuaValue::String(lua.create_string(&h.section_name)?)),
                    // lua.txt: 区間の数（中間点の数+1）。本実装は
                    // トラックバーの中間点を持たないため常に 1
                    // （レビュー round 2 Minor で是正。旧実装は
                    // ファイルのセクション数を返していた）。
                    "section_num" => Ok(LuaValue::Integer(1)),
                    "track_mode" => Ok(LuaValue::Integer(0)),
                    // lua.txt: 真偽値（旧実装は整数 0 で、Lua では
                    // 真として扱われてしまっていた）。
                    "gui" => Ok(LuaValue::Boolean(false)),
                    "blend" => Ok(LuaValue::String(lua.create_string(h.blend.name())?)),
                    "drawtarget" => {
                        Ok(LuaValue::String(lua.create_string(match h.drawtarget {
                            Target::Obj => "obj",
                            Target::Tmp => "tempbuffer",
                            Target::Frm => "framebuffer",
                        })?))
                    }
                    other => Err(unsupported(&format!("obj.getoption(\"{other}\")"))),
                }
            })?;
            obj.set("getoption", getoption)?;
        }
        {
            let host = host.clone();
            let getinfo = scope.create_function(move |lua, name: String| {
                let h = host.borrow();
                match name.as_str() {
                    // lua.txt: スクリプトフォルダのパス（旧実装は
                    // ファイル名を返していた）。
                    "script_path" => Ok(MultiValue::from_vec(vec![LuaValue::String(
                        lua.create_string(h.scripts_dir.to_string_lossy().as_ref())?,
                    )])),
                    "saving" => Ok(MultiValue::from_vec(vec![LuaValue::Boolean(false)])),
                    // lua.txt: (max_x, max_y) の2値を返す。
                    "image_max" => Ok(MultiValue::from_vec(vec![
                        LuaValue::Integer(4096),
                        LuaValue::Integer(4096),
                    ])),
                    other => Err(unsupported(&format!("obj.getinfo(\"{other}\")"))),
                }
            })?;
            obj.set("getinfo", getinfo)?;
        }

        // obj.getpixel / putpixel / copypixel / getpixeldata /
        // putpixeldata / pixeloption
        {
            let host = host.clone();
            let getpixel = scope.create_function(move |_, args: MultiValue| {
                let values: Vec<LuaValue> = args.into_iter().collect();
                let opt_num = |i: usize| -> Option<f64> {
                    match values.get(i) {
                        Some(LuaValue::Number(n)) => Some(*n),
                        Some(LuaValue::Integer(n)) => Some(*n as f64),
                        _ => None,
                    }
                };
                let h = host.borrow();
                // 読み取り先は pixeloption("get",...) の状態
                // （既定はオブジェクトバッファ）。
                let buf = if h.pixel_get_frm { &h.canvas } else { &h.objbuf };
                // lua.txt: 引数なしのときは (width, height) を返す。
                let (Some(x), Some(y)) = (opt_num(0), opt_num(1)) else {
                    return Ok(MultiValue::from_vec(vec![
                        LuaValue::Integer(buf.width as i64),
                        LuaValue::Integer(buf.height as i64),
                    ]));
                };
                // 第3引数 type があればその場の形式、無ければ
                // pixeloption の状態。形式は "col"/"rgb"/"yc"。
                let rgb = match values.get(2) {
                    Some(LuaValue::String(s)) => match s.to_string_lossy().as_ref() {
                        "col" => false,
                        "rgb" => true,
                        "yc" => {
                            return Err(unsupported(
                                "obj.getpixel の type=\"yc\"（YC 形式）",
                            ))
                        }
                        other => {
                            return Err(mlua::Error::runtime(format!(
                                "obj.getpixel: 未知のピクセル形式です: {other}"
                            )))
                        }
                    },
                    _ => h.pixel_rgb,
                };
                let px = buf
                    .get(x.round() as i64, y.round() as i64)
                    .unwrap_or([0, 0, 0, 0]);
                if rgb {
                    // rgb 形式: (r, g, b, a)。a は 0〜255。
                    Ok(MultiValue::from_vec(vec![
                        LuaValue::Integer(px[0] as i64),
                        LuaValue::Integer(px[1] as i64),
                        LuaValue::Integer(px[2] as i64),
                        LuaValue::Integer(px[3] as i64),
                    ]))
                } else {
                    // col 形式: (col, alpha)。alpha は 0.0〜1.0 の
                    // 実数（レビュー round 2 Major で是正。旧実装は
                    // col だけを返し、αを捨てていた）。
                    Ok(MultiValue::from_vec(vec![
                        LuaValue::Integer(pack_color(
                            px[0] as f64,
                            px[1] as f64,
                            px[2] as f64,
                        )),
                        LuaValue::Number(px[3] as f64 / 255.0),
                    ]))
                }
            })?;
            obj.set("getpixel", getpixel)?;
        }
        {
            let host = host.clone();
            let putpixel = scope.create_function(move |_, args: MultiValue| {
                let values: Vec<LuaValue> = args.into_iter().collect();
                let opt_num = |i: usize| -> Option<f64> {
                    match values.get(i) {
                        Some(LuaValue::Number(n)) => Some(*n),
                        Some(LuaValue::Integer(n)) => Some(*n as f64),
                        _ => None,
                    }
                };
                let num = |i: usize| -> mlua::Result<f64> {
                    opt_num(i).ok_or_else(|| {
                        mlua::Error::runtime("obj.putpixel: 数値が必要です")
                    })
                };
                let mut h = host.borrow_mut();
                let (tw, th) = if h.pixel_put_frm {
                    (h.canvas.width, h.canvas.height)
                } else {
                    (h.objbuf.width, h.objbuf.height)
                };
                // lua.txt: 引数なしのときは (width, height) を返す。
                if values.is_empty() {
                    return Ok(MultiValue::from_vec(vec![
                        LuaValue::Integer(tw as i64),
                        LuaValue::Integer(th as i64),
                    ]));
                }
                let (x, y) = (num(0)?.round() as i64, num(1)?.round() as i64);
                let rgba = if h.pixel_rgb {
                    // rgb 形式: (x, y, r, g, b [, a])。a は 0〜255。
                    [
                        num(2)?.round().clamp(0.0, 255.0) as u8,
                        num(3)?.round().clamp(0.0, 255.0) as u8,
                        num(4)?.round().clamp(0.0, 255.0) as u8,
                        opt_num(5)
                            .map(|a| a.round().clamp(0.0, 255.0) as u8)
                            .unwrap_or(255),
                    ]
                } else {
                    // col 形式: (x, y, col [, a])。a は 0.0〜1.0。
                    let color = num(2)? as u32;
                    [
                        ((color >> 16) & 0xFF) as u8,
                        ((color >> 8) & 0xFF) as u8,
                        (color & 0xFF) as u8,
                        opt_num(3)
                            .map(|a| (a.clamp(0.0, 1.0) * 255.0).round() as u8)
                            .unwrap_or(255),
                    ]
                };
                let blend_mode = h.pixel_blend;
                let target = if h.pixel_put_frm {
                    &mut h.canvas
                } else {
                    &mut h.objbuf
                };
                match blend_mode {
                    Some(mode) => {
                        if let Some(dst) = target.get(x, y) {
                            let mut px = dst;
                            blend_pixel(&mut px, rgba, 1.0, mode);
                            target.set(x, y, px);
                        }
                    }
                    None => target.set(x, y, rgba),
                }
                if h.pixel_put_frm {
                    h.drawn = true;
                }
                Ok(MultiValue::new())
            })?;
            obj.set("putpixel", putpixel)?;
        }
        {
            let host = host.clone();
            let copypixel =
                scope.create_function(move |_, (x1, y1, x2, y2): (f64, f64, f64, f64)| {
                    let mut h = host.borrow_mut();
                    if let Some(px) = h.objbuf.get(x2.round() as i64, y2.round() as i64) {
                        h.objbuf.set(x1.round() as i64, y1.round() as i64, px);
                    }
                    Ok(())
                })?;
            obj.set("copypixel", copypixel)?;
        }
        {
            let host = host.clone();
            let getpixeldata = scope.create_function(move |lua, ()| {
                let h = host.borrow();
                lua.create_string(&h.objbuf.data)
            })?;
            obj.set("getpixeldata", getpixeldata)?;
        }
        {
            let host = host.clone();
            let putpixeldata = scope.create_function(move |_, data: LuaString| {
                let bytes = data.as_bytes();
                let mut h = host.borrow_mut();
                if bytes.len() != h.objbuf.data.len() {
                    return Err(mlua::Error::runtime(format!(
                        "obj.putpixeldata: データ長 {} がバッファ {}x{} の {} バイトと一致しません",
                        bytes.len(),
                        h.objbuf.width,
                        h.objbuf.height,
                        h.objbuf.data.len()
                    )));
                }
                h.objbuf.data.copy_from_slice(&bytes);
                Ok(())
            })?;
            obj.set("putpixeldata", putpixeldata)?;
        }
        {
            let host = host.clone();
            let pixeloption = scope.create_function(move |_, args: MultiValue| {
                let values: Vec<LuaValue> = args.into_iter().collect();
                // lua.txt: obj.pixeloption(name, value) の2引数形式。
                // name は "type" / "get" / "put" / "blend"。
                let name = match values.first() {
                    Some(LuaValue::String(s)) => s.to_string_lossy(),
                    _ => {
                        return Err(mlua::Error::runtime(
                            "obj.pixeloption: (name, value) の2引数が必要です",
                        ))
                    }
                };
                let value = values.get(1).cloned().unwrap_or(LuaValue::Nil);
                let mut h = host.borrow_mut();
                match name.as_str() {
                    "type" => match &value {
                        LuaValue::String(s) => match s.to_string_lossy().as_ref() {
                            "col" => h.pixel_rgb = false,
                            "rgb" => h.pixel_rgb = true,
                            "yc" => {
                                return Err(unsupported(
                                    "obj.pixeloption(\"type\",\"yc\")（YC 形式）",
                                ))
                            }
                            other => {
                                return Err(mlua::Error::runtime(format!(
                                    "obj.pixeloption: 未知のピクセル形式です: {other}"
                                )))
                            }
                        },
                        _ => {
                            return Err(mlua::Error::runtime(
                                "obj.pixeloption(\"type\", ...): 形式名（文字列）が必要です",
                            ))
                        }
                    },
                    "get" | "put" => match &value {
                        LuaValue::String(s) => match s.to_string_lossy().as_ref() {
                            "obj" => {
                                if name == "get" {
                                    h.pixel_get_frm = false;
                                } else {
                                    h.pixel_put_frm = false;
                                }
                            }
                            "frm" => {
                                if name == "get" {
                                    h.pixel_get_frm = true;
                                } else {
                                    h.pixel_put_frm = true;
                                }
                            }
                            other => {
                                return Err(mlua::Error::runtime(format!(
                                    "obj.pixeloption(\"{name}\", ...): 対象は \"obj\" / \"frm\" です: {other}"
                                )))
                            }
                        },
                        _ => {
                            return Err(mlua::Error::runtime(
                                "obj.pixeloption: 対象名（文字列）が必要です",
                            ))
                        }
                    },
                    "blend" => {
                        // nil = そのまま上書き、それ以外は合成モード
                        // （数値または名前）。
                        h.pixel_blend = match &value {
                            LuaValue::Nil => None,
                            LuaValue::String(s) => {
                                Some(Blend::from_name(&decode_sjis(&s.as_bytes())).ok_or_else(
                                    || {
                                        mlua::Error::runtime(
                                            "obj.pixeloption(\"blend\", ...): 未知の合成モードです",
                                        )
                                    },
                                )?)
                            }
                            LuaValue::Integer(i) => Some(Blend::from_index(*i).ok_or_else(
                                || {
                                    mlua::Error::runtime(
                                        "obj.pixeloption(\"blend\", ...): 未知の合成モードです",
                                    )
                                },
                            )?),
                            LuaValue::Number(n) => Some(
                                Blend::from_index(*n as i64).ok_or_else(|| {
                                    mlua::Error::runtime(
                                        "obj.pixeloption(\"blend\", ...): 未知の合成モードです",
                                    )
                                })?,
                            ),
                            _ => {
                                return Err(mlua::Error::runtime(
                                    "obj.pixeloption(\"blend\", ...): nil / 数値 / 名前が必要です",
                                ))
                            }
                        };
                    }
                    other => {
                        return Err(mlua::Error::runtime(format!(
                            "obj.pixeloption: 未知のオプションです: {other}"
                        )))
                    }
                }
                Ok(())
            })?;
            obj.set("pixeloption", pixeloption)?;
        }

        // obj.rand / obj.setfont / obj.mes
        {
            let host = host.clone();
            let rand = scope.create_function(move |_, args: MultiValue| {
                let values: Vec<f64> = args
                    .into_iter()
                    .filter_map(|v| match v {
                        LuaValue::Number(n) => Some(n),
                        LuaValue::Integer(i) => Some(i as f64),
                        _ => None,
                    })
                    .collect();
                let mut h = host.borrow_mut();
                let (lo, hi) = match values.len() {
                    1 => (0.0, values[0]),
                    2 => (values[0], values[1]),
                    _ => {
                        return Err(mlua::Error::runtime(
                            "obj.rand: 引数は (最大値) か (最小値, 最大値) です",
                        ))
                    }
                };
                let span = (hi - lo).max(0.0) as u64 + 1;
                let picked = (h.next_rand() % span) as f64 + lo;
                Ok(picked)
            })?;
            obj.set("rand", rand)?;
        }
        {
            let host = host.clone();
            let setfont = scope.create_function(move |_, args: MultiValue| {
                let values: Vec<LuaValue> = args.into_iter().collect();
                let num = |i: usize| -> Option<f64> {
                    match values.get(i) {
                        Some(LuaValue::Number(n)) => Some(*n),
                        Some(LuaValue::Integer(n)) => Some(*n as f64),
                        _ => None,
                    }
                };
                let flag = |i: usize| -> Option<bool> {
                    match values.get(i) {
                        Some(LuaValue::Number(n)) => Some(*n != 0.0),
                        Some(LuaValue::Integer(n)) => Some(*n != 0),
                        Some(LuaValue::Boolean(b)) => Some(*b),
                        _ => None,
                    }
                };
                // ExEdit の署名は (name, size, type, col1, col2,
                // bold, italic, charspacing, linespacing)。
                // col1 が文字色、col2 が影・縁の色（レビュー round 1
                // Major で是正。旧対応付けは arg2/3/4 を太字/斜体/
                // 色と誤っていた）。
                let mut h = host.borrow_mut();
                if let Some(size) = num(1) {
                    h.font.size = size;
                }
                if let Some(kind) = num(2) {
                    h.font.decoration = kind as u32;
                }
                if let Some(color) = num(3) {
                    h.font.color = color as u32;
                }
                if let Some(color) = num(4) {
                    h.font.color2 = color as u32;
                }
                if let Some(bold) = flag(5) {
                    h.font.bold = bold;
                }
                if let Some(italic) = flag(6) {
                    h.font.italic = italic;
                }
                if let Some(sp) = num(7) {
                    h.font.spacing_x = sp;
                }
                if let Some(sp) = num(8) {
                    h.font.spacing_y = sp;
                }
                Ok(())
            })?;
            obj.set("setfont", setfont)?;
        }
        {
            let host = host.clone();
            let mes = scope.create_function(move |_, text: LuaString| {
                let decoded = decode_sjis(&text.as_bytes());
                let mut h = host.borrow_mut();
                h.objbuf = text_image(&decoded, &h.font).map_err(mlua::Error::runtime)?;
                drop(h);
                host.borrow().sync_obj_dims();
                Ok(())
            })?;
            obj.set("mes", mes)?;
        }

        // 未対応スタブ（黙って壊さず明示エラーにする）。
        for (name, what) in [
            ("getvalue", "obj.getvalue（他オブジェクトの値取得）"),
            ("getaudio", "obj.getaudio（音声データ取得）"),
            ("getpoint", "obj.getpoint（アンカーポイント取得）"),
            ("setanchor", "obj.setanchor（アンカーポイント設定）"),
            ("interpolation", "obj.interpolation（補間移動）"),
        ] {
            let stub =
                scope.create_function(move |_, _: MultiValue| Err::<(), _>(unsupported(what)))?;
            obj.set(name, stub)?;
        }

        globals.set("obj", obj)?;

        // --- グローバル: RGB / HSV / require / dialog 変数等 ---
        // RGB / HSV: lua.txt の全形式に対応する。
        // RGB(col) → (r,g,b) / RGB(r,g,b) → col /
        // RGB(r1,g1,b1,r2,g2,b2) → オブジェクトの時間進行で補間。
        // HSV も同様（1引数で (h,s,v) を返す。h は 0〜360、
        // s/v は 0〜100）。
        {
            let host_rgb = host.clone();
            let host_hsv = host.clone();
            let time_ratio = |h: &Host| -> f64 {
                let total = h.obj_num("totaltime", 0.0);
                if total > 0.0 {
                    (h.obj_num("time", 0.0) / total).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            };
            fn arg_nums(args: MultiValue) -> Vec<f64> {
                args.into_iter()
                    .filter_map(|v| match v {
                        LuaValue::Number(n) => Some(n),
                        LuaValue::Integer(i) => Some(i as f64),
                        _ => None,
                    })
                    .collect()
            }
            let rgb = scope.create_function(move |_, args: MultiValue| {
                let values = arg_nums(args);
                match values.len() {
                    1 => {
                        let col = values[0] as u32;
                        Ok(MultiValue::from_vec(vec![
                            LuaValue::Integer(((col >> 16) & 0xFF) as i64),
                            LuaValue::Integer(((col >> 8) & 0xFF) as i64),
                            LuaValue::Integer((col & 0xFF) as i64),
                        ]))
                    }
                    3 => Ok(MultiValue::from_vec(vec![LuaValue::Integer(pack_color(
                        values[0], values[1], values[2],
                    ))])),
                    6 => {
                        let t = time_ratio(&host_rgb.borrow());
                        let lerp = |a: f64, b: f64| a + (b - a) * t;
                        Ok(MultiValue::from_vec(vec![LuaValue::Integer(pack_color(
                            lerp(values[0], values[3]),
                            lerp(values[1], values[4]),
                            lerp(values[2], values[5]),
                        ))]))
                    }
                    n => Err(mlua::Error::runtime(format!(
                        "RGB: 引数は1（逆変換）/3/6（グラデーション）個です（{n} 個）"
                    ))),
                }
            })?;
            globals.set("RGB", rgb)?;
            let hsv = scope.create_function(move |_, args: MultiValue| {
                let values = arg_nums(args);
                match values.len() {
                    1 => {
                        let col = values[0] as u32;
                        let (h, s, v) = rgb_to_hsv(
                            ((col >> 16) & 0xFF) as f64 / 255.0,
                            ((col >> 8) & 0xFF) as f64 / 255.0,
                            (col & 0xFF) as f64 / 255.0,
                        );
                        Ok(MultiValue::from_vec(vec![
                            LuaValue::Number(h),
                            LuaValue::Number(s * 100.0),
                            LuaValue::Number(v * 100.0),
                        ]))
                    }
                    3 => {
                        let (r, g, b) = hsv_to_rgb(
                            values[0],
                            (values[1] / 100.0).clamp(0.0, 1.0),
                            (values[2] / 100.0).clamp(0.0, 1.0),
                        );
                        Ok(MultiValue::from_vec(vec![LuaValue::Integer(pack_color(
                            r * 255.0,
                            g * 255.0,
                            b * 255.0,
                        ))]))
                    }
                    6 => {
                        let t = time_ratio(&host_hsv.borrow());
                        let lerp = |a: f64, b: f64| a + (b - a) * t;
                        let (r, g, b) = hsv_to_rgb(
                            lerp(values[0], values[3]),
                            (lerp(values[1], values[4]) / 100.0).clamp(0.0, 1.0),
                            (lerp(values[2], values[5]) / 100.0).clamp(0.0, 1.0),
                        );
                        Ok(MultiValue::from_vec(vec![LuaValue::Integer(pack_color(
                            r * 255.0,
                            g * 255.0,
                            b * 255.0,
                        ))]))
                    }
                    n => Err(mlua::Error::runtime(format!(
                        "HSV: 引数は1（逆変換）/3/6（グラデーション）個です（{n} 個）"
                    ))),
                }
            })?;
            globals.set("HSV", hsv)?;
        }
        {
            let scripts_dir = host.borrow().scripts_dir.clone();
            let loaded = lua.create_table()?;
            globals.set("__anm_loaded", loaded.clone())?;
            let require = scope.create_function(move |lua, name: String| {
                if let Ok(cached) = loaded.get::<LuaValue>(name.as_str()) {
                    if cached != LuaValue::Nil {
                        return Ok(cached);
                    }
                }
                let candidates = [
                    scripts_dir.join(format!("{name}.lua")),
                    scripts_dir.join(format!("{}.lua", name.replace('.', "/"))),
                ];
                // フォルダ外への脱出（`../` 等）は許可しない
                // （docs/aviutl-compat.md の境界。レビュー round 1
                // Minor で是正）。実在する候補がフォルダ外なら
                // 明示エラー、フォルダ内ならそれを使う。
                let dir_canon = scripts_dir
                    .canonicalize()
                    .unwrap_or_else(|_| scripts_dir.clone());
                let mut found: Option<PathBuf> = None;
                let mut escaped = false;
                for cand in &candidates {
                    if !cand.is_file() {
                        continue;
                    }
                    match cand.canonicalize() {
                        Ok(canon) if canon.starts_with(&dir_canon) => {
                            found = Some(cand.clone());
                            break;
                        }
                        _ => escaped = true,
                    }
                }
                if escaped && found.is_none() {
                    return Err(mlua::Error::runtime(format!(
                        "require できません: {name}（スクリプトフォルダ外のモジュールは読み込めません）"
                    )));
                }
                let Some(path) = found else {
                    return Err(mlua::Error::runtime(format!(
                        "require できません: {name}（スクリプトフォルダに {name}.lua がありません）"
                    )));
                };
                let bytes = std::fs::read(&path).map_err(|e| {
                    mlua::Error::runtime(format!("require: {} を読めません: {e}", path.display()))
                })?;
                let value: LuaValue = lua.load(&bytes).set_name(&name).eval()?;
                let stored = if value == LuaValue::Nil {
                    LuaValue::Boolean(true)
                } else {
                    value.clone()
                };
                loaded.set(name.as_str(), stored)?;
                Ok(value)
            })?;
            globals.set("require", require)?;
        }
        for (name, value) in &dialog_values {
            match value {
                ScriptParam::Number(n) => globals.set(name.as_str(), *n)?,
                ScriptParam::Bool(b) => globals.set(name.as_str(), *b)?,
                ScriptParam::Text(t) => {
                    globals.set(name.as_str(), lua.create_string(t)?)?;
                }
            }
        }
        if section.color_default.is_some() || params.contains_key("color") {
            globals.set("color", color_value)?;
        }
        if section.has_file {
            globals.set("file", lua.create_string(&file_value)?)?;
        }

        // --- 実行: --param: 注入 → セクション本体 ---
        if !trimmed(&param_code).is_empty() {
            lua.load(&param_code).set_name("param").eval::<LuaValue>()?;
        }
        lua.load(&section_body)
            .set_name(if section.name.is_empty() {
                "anm"
            } else {
                &section.name
            })
            .eval::<LuaValue>()?;
        Ok(())
    });
    lua.remove_hook();
    engine.check_limit(&exceeded)?;
    if let Err(e) = result {
        return Err(classify_error(e));
    }

    // 描画されなかった場合はオブジェクトバッファを重ねて確定する。
    let mut h = host.borrow_mut();
    if !h.drawn {
        let objbuf = h.objbuf.clone();
        blit_over(&mut h.canvas, &objbuf);
    }
    pixels.copy_from_slice(&h.canvas.data);
    Ok(())
}

/// copybuffer の読み取り元を解決する。
/// スクリプトフォルダ相対パスを、フォルダ内に収まる実パスへ
/// 解決する。`../` 遡りや絶対パスでの脱出は明示エラーにする
/// （require の包含チェックと境界をそろえる。画像読み込みは
/// レビュー round 3 Minor で是正）。
fn resolve_inside_scripts_dir(scripts_dir: &Path, rel: &str) -> Result<PathBuf, String> {
    let joined = scripts_dir.join(rel);
    let dir_canon = scripts_dir
        .canonicalize()
        .map_err(|e| format!("スクリプトフォルダを解決できません: {e}"))?;
    let canon = joined
        .canonicalize()
        .map_err(|_| format!("ファイルが開けません: {rel}"))?;
    if !canon.starts_with(&dir_canon) {
        return Err(format!(
            "スクリプトフォルダ外のファイルは読み込めません: {rel}"
        ));
    }
    Ok(canon)
}

fn resolve_buffer(h: &Host, name: &str, width: u32, height: u32) -> mlua::Result<RgbaImage> {
    match name {
        "obj" => Ok(h.objbuf.clone()),
        "tmp" => Ok(h
            .tempbuf
            .clone()
            .unwrap_or_else(|| RgbaImage::transparent(width, height))),
        "frm" => Ok(h.canvas.clone()),
        _ if name.starts_with("cache:") => h
            .cache
            .get(&name["cache:".len()..])
            .cloned()
            .ok_or_else(|| {
                mlua::Error::runtime(format!("obj.copybuffer: キャッシュが空です: {name}"))
            }),
        _ if name.starts_with("image:") => {
            let full = resolve_inside_scripts_dir(h.scripts_dir.as_path(), &name["image:".len()..])
                .map_err(mlua::Error::runtime)?;
            let img = image::open(&full).map_err(|e| {
                mlua::Error::runtime(format!(
                    "obj.copybuffer: {} を読めません: {e}",
                    full.display()
                ))
            })?;
            let rgba = img.to_rgba8();
            let (w, hh) = rgba.dimensions();
            Ok(RgbaImage::from_pixels(w, hh, rgba.into_raw()))
        }
        other => Err(mlua::Error::runtime(format!(
            "obj.copybuffer: 未知の読み取り元です: {other}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sjis(text: &str) -> Vec<u8> {
        encode_sjis(text)
    }

    #[test]
    fn sjis_roundtrip_japanese() {
        let bytes = encode_sjis("図形テスト");
        assert_eq!(decode_sjis(&bytes), "図形テスト");
    }

    #[test]
    fn parse_sections_and_directives() {
        let src = "@図形テスト\r\n--track0:大きさ,10,400,120\r\n--track1:横位置,-100,100,0,px\r\n--check0:表示する,1\r\n--dialog:倍率,scale=2;色指定/col,col=0xFF0000;名前,name=abc;図形名/fig,fig=\"円\"\r\n--color:0x00FF00\r\n--file:\r\n--param:base=10\r\nobj.load(\"figure\", \"円\", 0xff0000, obj.track0)\r\nobj.draw()\r\n@テキスト\r\nobj.mes(\"x\")\r\n";
        let file = parse_anm(&sjis(src));
        assert_eq!(file.sections.len(), 2);
        let s = &file.sections[0];
        assert_eq!(s.name, "図形テスト");
        let t0 = s.tracks[0].as_ref().unwrap();
        assert_eq!(t0.label, "大きさ");
        assert_eq!((t0.min, t0.max, t0.default), (10.0, 400.0, 120.0));
        let t1 = s.tracks[1].as_ref().unwrap();
        assert_eq!((t1.min, t1.max, t1.default), (-100.0, 100.0, 0.0));
        assert_eq!(s.check0, Some(("表示する".to_string(), true)));
        assert_eq!(s.dialog.len(), 4);
        assert_eq!(s.dialog[0].name, "scale");
        assert_eq!(s.dialog[0].kind, DialogKind::Value);
        assert_eq!(s.dialog[0].default, ScriptParam::Number(2.0));
        assert_eq!(s.dialog[1].kind, DialogKind::Color);
        // lua.txt の実形式: 接尾辞（/col 等）は表示名の最後に付き、
        // 図形名の初期値は引用符で囲まれる（引用符は外して保持）。
        assert_eq!(s.dialog[1].label, "色指定");
        assert_eq!(s.dialog[1].name, "col");
        assert_eq!(s.dialog[1].default, ScriptParam::Number(0xFF0000 as f64));
        assert_eq!(s.dialog[2].default, ScriptParam::Text("abc".into()));
        assert_eq!(s.dialog[3].kind, DialogKind::Figure);
        assert_eq!(s.dialog[3].label, "図形名");
        assert_eq!(s.dialog[3].name, "fig");
        assert_eq!(s.dialog[3].default, ScriptParam::Text("円".into()));
        assert_eq!(s.color_default, Some(0x00FF00 as f64));
        assert!(s.has_file);
        assert_eq!(decode_sjis(&s.param_code), "base=10");
        assert!(decode_sjis(&s.body).contains("obj.load"));
        assert_eq!(file.sections[1].name, "テキスト");
    }

    #[test]
    fn parse_file_starting_with_section_has_no_anonymous_section() {
        let file = parse_anm(&sjis("@のみ\nobj.draw()\n"));
        assert_eq!(file.sections.len(), 1);
        assert_eq!(file.sections[0].name, "のみ");
    }

    #[test]
    fn split_reference_uses_extension_at_sign() {
        assert_eq!(split_reference("fx.anm@図形"), ("fx.anm", Some("図形")));
        assert_eq!(split_reference("@配布.anm"), ("@配布.anm", None));
        assert_eq!(
            split_reference("@配布.anm@図形"),
            ("@配布.anm", Some("図形"))
        );
        assert_eq!(split_reference("fx.obj"), ("fx.obj", None));
    }

    #[test]
    fn check_anm_syntax_flags_broken_body() {
        let engine = ScriptEngine::new();
        let ok = sjis("@a\nobj.draw()\n");
        assert_eq!(check_anm_syntax(&engine, &ok), None);
        let broken = sjis("@a\nreturn 1 +\n");
        assert!(check_anm_syntax(&engine, &broken).is_some());
    }

    #[test]
    fn select_section_errors_for_unknown_name() {
        let file = parse_anm(&sjis("@a\nobj.draw()\n"));
        let err = select_section(&file, Some("無い")).unwrap_err();
        assert!(err.message.contains("見つかりません"), "{}", err.message);
    }

    // ---- 実行テスト ----

    fn run_sample(
        src: &str,
        section: Option<&str>,
        generated: bool,
        params: ScriptParams,
        size: u32,
        fill: [u8; 4],
    ) -> Result<Vec<u8>, ScriptError> {
        let engine = ScriptEngine::new();
        let mut pixels = Vec::new();
        for _ in 0..(size * size) {
            pixels.extend_from_slice(&fill);
        }
        let clip = ClipInfo {
            name: "clip".into(),
            duration_ms: 1000,
            source: None,
            time_ms: 0,
        };
        run_anm_effect(
            &engine,
            &sjis(src),
            section,
            generated,
            &clip,
            &params,
            30,
            size,
            size,
            &mut pixels,
            Path::new("/tmp"),
            "sample.anm",
        )?;
        Ok(pixels)
    }

    fn pixel(pixels: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let idx = ((y * size + x) * 4) as usize;
        [
            pixels[idx],
            pixels[idx + 1],
            pixels[idx + 2],
            pixels[idx + 3],
        ]
    }

    #[test]
    fn figure_sample_draws_red_circle() {
        let src = "@図形\n--track0:大きさ,10,400,120\nobj.load(\"figure\", \"円\", 0xff0000, obj.track0)\nobj.draw()\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out, 100, 50, 50), [255, 0, 0, 255]);
        assert_eq!(pixel(&out, 100, 0, 0), [0, 0, 0, 255]);
    }

    #[test]
    fn track_param_override_changes_rendering() {
        let src = "@図形\n--track0:大きさ,10,400,120\nobj.load(\"figure\", \"円\", 0xff0000, obj.track0)\nobj.draw()\n";
        let big = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&big, 100, 80, 50), [255, 0, 0, 255]);
        let mut params = ScriptParams::new();
        params.insert("track0".into(), ScriptParam::Number(40.0));
        let small = run_sample(src, None, false, params, 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&small, 100, 80, 50), [0, 0, 0, 255]);
        assert_eq!(pixel(&small, 100, 50, 50), [255, 0, 0, 255]);
    }

    #[test]
    fn text_sample_renders_glyph_pixels() {
        let src =
            "@文字\nobj.setfont(\"g\", 40, 0, 0xffffff)\nobj.load(\"text\", \"A\")\nobj.draw()\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        let bright = out
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] > 100 && px[1] > 100 && px[2] > 100)
            .count();
        assert!(bright > 20, "文字のピクセルが少なすぎる: {bright}");
    }

    #[test]
    fn unsupported_api_and_effect_error_explicitly() {
        let src = "@x\nobj.getvalue(0, \"x\")\n";
        let err =
            run_sample(src, None, false, ScriptParams::new(), 16, [0, 0, 0, 255]).unwrap_err();
        assert!(err.message.contains("未対応"), "{}", err.message);
        let src2 = "@x\nobj.load(\"object\")\nobj.effect(\"存在しない効果\")\n";
        let err2 =
            run_sample(src2, None, false, ScriptParams::new(), 16, [0, 0, 0, 255]).unwrap_err();
        assert!(err2.message.contains("未対応"), "{}", err2.message);
    }

    #[test]
    fn section_selection_runs_named_section() {
        let src = "@赤\nobj.load(\"figure\", \"四角形\", 0xff0000, 100)\nobj.draw()\n@青\nobj.load(\"figure\", \"四角形\", 0x0000ff, 100)\nobj.draw()\n";
        let out = run_sample(
            src,
            Some("青"),
            false,
            ScriptParams::new(),
            100,
            [0, 0, 0, 255],
        )
        .unwrap();
        assert_eq!(pixel(&out, 100, 50, 50), [0, 0, 255, 255]);
    }

    #[test]
    fn effect_without_draw_applies_to_object_buffer() {
        let src = "@反転\nobj.load(\"object\")\nobj.effect(\"色反転\")\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 16, [10, 20, 30, 255]).unwrap();
        assert_eq!(pixel(&out, 16, 3, 3), [245, 235, 225, 255]);
    }

    #[test]
    fn check0_and_dialog_globals_drive_script() {
        let src = "@条件\n--check0:有効,1\n--dialog:倍率,scale=1\nif obj.check0 then obj.load(\"figure\", \"円\", 0x00ff00, 40 * scale) obj.draw() end\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out, 100, 50, 50), [0, 255, 0, 255]);
        assert_eq!(pixel(&out, 100, 75, 50), [0, 0, 0, 255]);
        let mut params = ScriptParams::new();
        params.insert("scale".into(), ScriptParam::Number(2.0));
        let out2 = run_sample(src, None, false, params, 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out2, 100, 75, 50), [0, 255, 0, 255]);
        let mut params3 = ScriptParams::new();
        params3.insert("check0".into(), ScriptParam::Bool(false));
        let out3 = run_sample(src, None, false, params3, 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out3, 100, 50, 50), [0, 0, 0, 255]);
    }

    #[test]
    fn setfont_col1_is_the_text_color() {
        // レビュー round 1 Major: ExEdit の obj.setfont は
        // (name, size, type, col1, col2, bold, italic, ...) で、
        // col1 が文字色。旧マッピング（arg2=太字, arg3=斜体,
        // arg4=色）では 0xff0000 が斜体スロットに入り白で描かれた。
        let src =
            "@文字\nobj.setfont(\"g\", 40, 0, 0xff0000)\nobj.load(\"text\", \"A\")\nobj.draw()\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        let red = out
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] > 150 && px[1] < 100 && px[2] < 100)
            .count();
        assert!(red > 20, "col1 の赤で描かれるはず: red={red}");
        // 太字・斜体フラグ（arg5/arg6）が色を壊さないことも固定する。
        let src2 = "@文字\nobj.setfont(\"g\", 40, 0, 0xff0000, 0x000000, 1, 1)\nobj.load(\"text\", \"A\")\nobj.draw()\n";
        let out2 = run_sample(src2, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        let red2 = out2
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] > 150 && px[1] < 100 && px[2] < 100)
            .count();
        assert!(red2 > 20, "全引数指定でも col1 の赤のはず: red={red2}");
    }

    #[test]
    fn setfont_shadow_type_draws_col2_shadow() {
        // type=1（影付き）: col2 の影が col1 本体の右下に描かれる。
        let src = "@影\nobj.setfont(\"g\", 40, 1, 0xffffff, 0xff0000)\nobj.load(\"text\", \"A\")\nobj.draw()\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        let white = out
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] > 200 && px[1] > 200 && px[2] > 200)
            .count();
        let red = out
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] > 150 && px[1] < 100 && px[2] < 100)
            .count();
        assert!(white > 20, "本体は col1 の白のはず: white={white}");
        assert!(red > 5, "影は col2 の赤のはず: red={red}");
    }

    #[test]
    fn require_cannot_escape_scripts_dir() {
        // レビュー round 1 Minor: require のモジュール名で
        // スクリプトフォルダの外（../）を読めてはならない
        // （docs/aviutl-compat.md の「フォルダ内」境界どおり）。
        let root = std::env::temp_dir().join(format!("anm-req-escape-{}", std::process::id()));
        let scripts = root.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(root.join("outside.lua"), "escaped = true\n").unwrap();
        let engine = ScriptEngine::new();
        let mut pixels = vec![0u8; 4 * 4 * 4];
        let clip = ClipInfo {
            name: "clip".into(),
            duration_ms: 1000,
            source: None,
            time_ms: 0,
        };
        let err = run_anm_effect(
            &engine,
            &sjis("@x\nrequire(\"../outside\")\n"),
            None,
            false,
            &clip,
            &ScriptParams::new(),
            30,
            4,
            4,
            &mut pixels,
            &scripts,
            "sample.anm",
        )
        .unwrap_err();
        assert!(err.message.contains("フォルダ外"), "{}", err.message);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn require_loads_module_inside_scripts_dir() {
        // フォルダ内の require は従来どおり動く（包含チェックで
        // 正当な読み込みを壊さないことの固定）。
        let root = std::env::temp_dir().join(format!("anm-req-inside-{}", std::process::id()));
        let scripts = root.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("helper.lua"), "helper_color = 0x00ff00\n").unwrap();
        let engine = ScriptEngine::new();
        let mut pixels = Vec::new();
        for _ in 0..(100 * 100) {
            pixels.extend_from_slice(&[0u8, 0, 0, 255]);
        }
        let clip = ClipInfo {
            name: "clip".into(),
            duration_ms: 1000,
            source: None,
            time_ms: 0,
        };
        run_anm_effect(
            &engine,
            &sjis("@x\nrequire(\"helper\")\nobj.load(\"figure\", \"円\", helper_color, 60)\nobj.draw()\n"),
            None,
            false,
            &clip,
            &ScriptParams::new(),
            30,
            100,
            100,
            &mut pixels,
            &scripts,
            "sample.anm",
        )
        .unwrap();
        assert_eq!(pixel(&pixels, 100, 50, 50), [0, 255, 0, 255]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn pixeldata_roundtrip_is_identity() {
        let src = "@p\nlocal d = obj.getpixeldata()\nobj.putpixeldata(d)\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 8, [7, 8, 9, 255]).unwrap();
        assert_eq!(pixel(&out, 8, 2, 2), [7, 8, 9, 255]);
    }

    #[test]
    fn param_injection_runs_before_body() {
        let src = "@p\n--param:sz=30\nobj.load(\"figure\", \"四角形\", 0xffffff, sz)\nobj.draw()\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out, 100, 50, 50), [255, 255, 255, 255]);
        assert_eq!(pixel(&out, 100, 70, 50), [0, 0, 0, 255]);
    }

    // ---- レビュー round 2: 実形式（lua.txt）との対応付け是正 ----

    #[test]
    fn draw_uses_exedit_argument_order() {
        // lua.txt: obj.draw([ox,oy,oz,zoom,alpha,rx,ry,rz])。
        // 合成モード引数は無い（blend は setoption で指定）。
        // 旧対応付け（x,y,zoom,alpha,blend）では oz の 0 が
        // zoom に入り何も描かれなかった。
        let src = "@d\nobj.load(\"figure\", \"四角形\", 0xff0000, 100)\nobj.draw(0, 0, 0, 0.5)\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out, 100, 50, 50), [255, 0, 0, 255]);
        assert_eq!(pixel(&out, 100, 85, 50), [0, 0, 0, 255]);
        // alpha は第5引数: 0.5 で半分に混ざる。
        let src2 =
            "@d\nobj.load(\"figure\", \"四角形\", 0xff0000, 100)\nobj.draw(0, 0, 0, 1, 0.5)\n";
        let out2 = run_sample(src2, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out2, 100, 50, 50), [128, 0, 0, 255]);
    }

    #[test]
    fn drawpoly_uses_exedit_argument_order() {
        // lua.txt: obj.drawpoly(x0,y0,z0,...,x3,y3,z3
        //   [,u0,v0,...,u3,v3,alpha])。lua.txt の例そのままの
        // 呼び出しで、alpha（21番目）が効くこと。
        let src = "@d\nobj.load(\"figure\", \"四角形\", 0xff0000, 100)\nobj.drawpoly(-50,-50,0, 50,-50,0, 50,50,0, -50,50,0, 0,0, 100,0, 100,100, 0,100, 0.5)\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out, 100, 50, 50), [128, 0, 0, 255]);
    }

    #[test]
    fn blend_index_five_is_overlay_and_drawtarget_canonical_names() {
        // lua.txt の blend: 5=オーバーレイ（旧対応付けは
        // 5=比較（明）でずれていた）。dst=64, src=192 の
        // オーバーレイは 2*64*192/255 ≒ 96。
        let src = "@b\nobj.load(\"figure\", \"四角形\", 0xC0C0C0, 100)\nobj.setoption(\"blend\", 5)\nobj.draw()\n";
        let out = run_sample(
            src,
            None,
            false,
            ScriptParams::new(),
            100,
            [64, 64, 64, 255],
        )
        .unwrap();
        assert_eq!(pixel(&out, 100, 50, 50), [96, 96, 96, 255]);
        // drawtarget の正式名は "tempbuffer" / "framebuffer"。
        let src2 = "@b\nobj.load(\"figure\", \"四角形\", 0xff0000, 100)\nobj.setoption(\"drawtarget\", \"tempbuffer\")\nobj.draw()\nobj.setoption(\"drawtarget\", \"framebuffer\")\nobj.copybuffer(\"obj\", \"tmp\")\nobj.draw()\n";
        let out2 = run_sample(src2, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out2, 100, 50, 50), [255, 0, 0, 255]);
    }

    #[test]
    fn load_text_speed_is_not_a_color() {
        // lua.txt: obj.load("text",text[,speed,time])。第3引数は
        // 表示速度であって色ではない（旧実装は色として扱い、
        // speed=10 で文字が 0x00000A の暗色になっていた）。
        // 色は setfont の col1 のまま、speed*time 文字だけ出る。
        // （speed は 10.5 と小数で書く。整数リテラルだと旧実装の
        // 色上書きが発火しないため、回帰を確実に捕まえる形にする）
        let count_red = |out: &[u8]| {
            out.as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[0] > 150 && px[1] < 100 && px[2] < 100)
                .count()
        };
        // 比較基準: speed 指定なしの "AB" 全体（同じ環境・同じ
        // フォントで描いた絶対量。ピクセル数の絶対閾値は環境の
        // フォント差で境界落ちしたため、相対比較にする）。
        let full_src =
            "@t\nobj.setfont(\"g\", 40, 0, 0xff0000)\nobj.load(\"text\", \"AB\")\nobj.draw()\n";
        let full = run_sample(
            full_src,
            None,
            false,
            ScriptParams::new(),
            100,
            [0, 0, 0, 255],
        )
        .unwrap();
        let full_red = count_red(&full);
        let src = "@t\nobj.setfont(\"g\", 40, 0, 0xff0000)\nobj.load(\"text\", \"AB\", 10.5, 0.15)\nobj.draw()\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        let red = count_red(&out);
        assert!(red > 20, "setfont の赤で描かれるはず: red={red}");
        // 2文字分の 3/4 未満 = 1文字分だけのはず。
        assert!(
            red * 4 < full_red * 3,
            "speed*time で1文字に制限されるはず: red={red} full={full_red}"
        );
    }

    #[test]
    fn pixel_apis_follow_lua_txt() {
        // getpixel: col 形式は (色, 不透明度0.0-1.0)、引数なしは
        // (幅, 高さ)。pixeloption は ("type","rgb") の2引数形式で
        // rgb 形式は (r,g,b,a)。putpixel の第4引数は不透明度。
        let src = "@p\nobj.load(\"figure\", \"四角形\", 0xff0000, 100)\nlocal col, a = obj.getpixel(10, 10)\nlocal w, h = obj.getpixel()\nobj.pixeloption(\"type\", \"rgb\")\nlocal r, g, b, a2 = obj.getpixel(10, 10)\nobj.pixeloption(\"type\", \"col\")\nobj.putpixel(10, 10, 0x00ff00, 0.5)\nlocal col2, a3 = obj.getpixel(10, 10)\nif col == 0xff0000 and a == 1.0 and w == 100 and h == 100 and r == 255 and g == 0 and b == 0 and a2 == 255 and col2 == 0x00ff00 and a3 > 0.49 and a3 < 0.51 then obj.load(\"figure\", \"円\", 0x0000ff, 100) end\nobj.draw()\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out, 100, 50, 50), [0, 0, 255, 255]);
    }

    #[test]
    fn getoption_getinfo_follow_lua_txt() {
        // gui は真偽値、section_num は区間の数（中間点+1。本実装は
        // 中間点を持たないため 1）、script_path はフォルダのパス、
        // image_max は (max_x, max_y) の2値。
        let src = "@i\nlocal path = obj.getinfo(\"script_path\")\nlocal mx, my = obj.getinfo(\"image_max\")\nif obj.getoption(\"gui\") == false and obj.getoption(\"section_num\") == 1 and path == \"/tmp\" and mx == 4096 and my == 4096 then obj.load(\"figure\", \"円\", 0x0000ff, 100) end\nobj.draw()\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out, 100, 50, 50), [0, 0, 255, 255]);
    }

    #[test]
    fn figure_line_draws_outline_only() {
        // lua.txt: obj.load("figure",name[,color,size,line]) の
        // line はライン幅。指定時は輪郭だけが描かれる。
        let src = "@f\nobj.load(\"figure\", \"四角形\", 0xff0000, 100, 10)\nobj.draw()\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out, 100, 50, 50), [0, 0, 0, 255]);
        assert_eq!(pixel(&out, 100, 5, 50), [255, 0, 0, 255]);
        assert_eq!(pixel(&out, 100, 95, 50), [255, 0, 0, 255]);
    }

    #[test]
    fn copybuffer_returns_true() {
        // lua.txt: obj.copybuffer の戻り値は true=成功。
        let src = "@c\nobj.load(\"figure\", \"四角形\", 0xff0000, 100)\nlocal ok = obj.copybuffer(\"tmp\", \"obj\")\nif ok == true then obj.load(\"figure\", \"円\", 0x0000ff, 100) end\nobj.draw()\n";
        let out = run_sample(src, None, false, ScriptParams::new(), 100, [0, 0, 0, 255]).unwrap();
        assert_eq!(pixel(&out, 100, 50, 50), [0, 0, 255, 255]);
    }

    #[test]
    fn load_image_cannot_escape_scripts_dir() {
        // レビュー round 3 Minor: obj.load("image") も require と
        // 同じくスクリプトフォルダ外の画像を読めてはならない
        // （docs/aviutl-compat.md は「スクリプトフォルダ相対」と
        // 明記している）。
        let root = std::env::temp_dir().join(format!("anm-img-escape-{}", std::process::id()));
        let scripts = root.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(root.join("secret.png"))
            .unwrap();
        let engine = ScriptEngine::new();
        let mut pixels = vec![0u8; 4 * 4 * 4];
        let clip = ClipInfo {
            name: "clip".into(),
            duration_ms: 1000,
            source: None,
            time_ms: 0,
        };
        let err = run_anm_effect(
            &engine,
            &sjis("@x\nobj.load(\"image\", \"../secret.png\")\n"),
            None,
            false,
            &clip,
            &ScriptParams::new(),
            30,
            4,
            4,
            &mut pixels,
            &scripts,
            "sample.anm",
        )
        .unwrap_err();
        assert!(err.message.contains("フォルダ外"), "{}", err.message);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn copybuffer_image_cannot_escape_scripts_dir() {
        // レビュー round 3 Minor: copybuffer の image: 読み取り元
        // も同じ境界に従う。
        let root = std::env::temp_dir().join(format!("anm-cb-escape-{}", std::process::id()));
        let scripts = root.join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(root.join("secret.png"))
            .unwrap();
        let engine = ScriptEngine::new();
        let mut pixels = vec![0u8; 4 * 4 * 4];
        let clip = ClipInfo {
            name: "clip".into(),
            duration_ms: 1000,
            source: None,
            time_ms: 0,
        };
        let err = run_anm_effect(
            &engine,
            &sjis("@x\nobj.copybuffer(\"obj\", \"image:../secret.png\")\n"),
            None,
            false,
            &clip,
            &ScriptParams::new(),
            30,
            4,
            4,
            &mut pixels,
            &scripts,
            "sample.anm",
        )
        .unwrap_err();
        assert!(err.message.contains("フォルダ外"), "{}", err.message);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn rgb_hsv_reverse_and_gradient_forms() {
        // lua.txt: RGB/HSV は相互変換（1引数で要素を返す）と
        // 6引数のグラデーション（オブジェクト時間の割合で補間）。
        let engine = ScriptEngine::new();
        let mut pixels = Vec::new();
        for _ in 0..(100 * 100) {
            pixels.extend_from_slice(&[0u8, 0, 0, 255]);
        }
        let clip = ClipInfo {
            name: "clip".into(),
            duration_ms: 1000,
            source: None,
            time_ms: 500,
        };
        run_anm_effect(
            &engine,
            &sjis("@c\nlocal r, g, b = RGB(0xff8000)\nlocal h, s, v = HSV(0xff0000)\nlocal grad = RGB(0, 0, 0, 255, 255, 255)\nif r == 255 and g == 128 and b == 0 and h == 0 and s == 100 and v == 100 and grad == 0x808080 then obj.load(\"figure\", \"円\", 0x0000ff, 100) end\nobj.draw()\n"),
            None,
            false,
            &clip,
            &ScriptParams::new(),
            30,
            100,
            100,
            &mut pixels,
            Path::new("/tmp"),
            "sample.anm",
        )
        .unwrap();
        assert_eq!(pixel(&pixels, 100, 50, 50), [0, 0, 255, 255]);
    }
}
