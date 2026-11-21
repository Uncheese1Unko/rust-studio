//! DAW のビジュアルテーマ（Issue #23）。
//!
//! 色の唯一の出所はデザイン文書 `docs/design/daw-visual-design.md` の
//! 配色表（ADR-0012）。描画コードは色リテラルを直接書かず、ここの
//! 定数だけを使う。テーマは DAW アプリ内に閉じる（studio-core には
//! 置かない：コアは GUI に依存しない）。

/// RGB 色（0.0–1.0）。nannou の `rgb()` にそのまま渡す。
pub type Rgb = (f32, f32, f32);

/// 基調・背景：ウォームクリーム `#FBF4EC`。
pub const BACKGROUND: Rgb = (0.984, 0.957, 0.925);
/// パネル面：ペールクリーム `#FFFDF9`。
pub const PANEL: Rgb = (1.000, 0.992, 0.976);
/// パネル面（沈み・バー背景）：サンド `#F3E7D9`。
pub const PANEL_SUNK: Rgb = (0.953, 0.906, 0.851);
/// 区切り線・細線：グレージュ線 `#E0D2C2`。
pub const LINE: Rgb = (0.878, 0.824, 0.761);
/// 文字・プレイヘッド：ウォームブラック `#211812`。
pub const TEXT: Rgb = (0.129, 0.094, 0.071);
/// 補助文字・非アクティブ：グレージュ `#A89880`。
pub const TEXT_SUB: Rgb = (0.659, 0.596, 0.502);
/// アクセント：ホットピンク `#FF2E88`。「今」のものだけに使う。
pub const ACCENT: Rgb = (1.000, 0.180, 0.533);
/// アクセント淡（クリップ面）：ソフトピンク `#FFC7DE`。
pub const ACCENT_SOFT: Rgb = (1.000, 0.780, 0.871);
/// 副色：ティール `#12B5A5`。
pub const TEAL: Rgb = (0.071, 0.710, 0.647);
/// 副色淡（クリップ面）：ミント `#C4EDE6`。
pub const MINT: Rgb = (0.769, 0.929, 0.902);

/// パネルラベル（連番＋大文字、デザイン文書「タイポグラフィと
/// パネルラベル」）。パネル左上に小さくグレージュで置く。
pub const TRANSPORT_LABEL: &str = "01 TRANSPORT";
pub const TIMELINE_LABEL: &str = "02 TIMELINE";
pub const MIXER_LABEL: &str = "03 MIXER";
pub const SAMPLER_LABEL: &str = "04 SAMPLER";

/// パネルラベルの文字サイズ（px、デザイン文書の目安 10–11px）。
pub const LABEL_FONT_SIZE: u32 = 11;
/// 「現在の値」の文字サイズ（px、デザイン文書の目安 20px 以上）。
pub const VALUE_FONT_SIZE: u32 = 20;
/// パネルの角丸半径（px、デザイン文書の目安）。
pub const PANEL_CORNER_RADIUS: f32 = 6.0;
/// チップ・クリップの角丸半径（px、デザイン文書の目安）。
pub const CHIP_CORNER_RADIUS: f32 = 4.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// (R, G, B) をデザイン文書の HEX 表記に戻す。
    fn to_hex(c: (f32, f32, f32)) -> String {
        format!(
            "#{:02X}{:02X}{:02X}",
            (c.0 * 255.0).round() as u8,
            (c.1 * 255.0).round() as u8,
            (c.2 * 255.0).round() as u8
        )
    }

    #[test]
    fn theme_palette_matches_design_doc() {
        // デザイン文書「配色」表の全色が定数として定義され、値が
        // 一致すること（Issue #23 の受け入れ条件）。
        let cases = [
            (BACKGROUND, "#FBF4EC", "基調・背景"),
            (PANEL, "#FFFDF9", "パネル面"),
            (PANEL_SUNK, "#F3E7D9", "パネル面（沈み・バー背景）"),
            (LINE, "#E0D2C2", "区切り線・細線"),
            (TEXT, "#211812", "文字・プレイヘッド"),
            (TEXT_SUB, "#A89880", "補助文字・非アクティブ"),
            (ACCENT, "#FF2E88", "アクセント"),
            (ACCENT_SOFT, "#FFC7DE", "アクセント淡（クリップ面）"),
            (TEAL, "#12B5A5", "副色"),
            (MINT, "#C4EDE6", "副色淡（クリップ面）"),
        ];
        for (value, hex, role) in cases {
            assert_eq!(to_hex(value), hex, "{role} の色が配色表と違う");
        }
    }

    #[test]
    fn panel_labels_follow_design_doc() {
        // デザイン文書「タイポグラフィとパネルラベル」: 連番＋大文字。
        assert_eq!(TRANSPORT_LABEL, "01 TRANSPORT");
        assert_eq!(TIMELINE_LABEL, "02 TIMELINE");
        assert_eq!(MIXER_LABEL, "03 MIXER");
        assert_eq!(SAMPLER_LABEL, "04 SAMPLER");
    }
}
