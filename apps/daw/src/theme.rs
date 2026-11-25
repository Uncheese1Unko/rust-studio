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
pub const COLOR_LABEL: &str = "05 COLOR";
pub const PIANO_ROLL_LABEL: &str = "06 PIANO ROLL";
pub const TONES_LABEL: &str = "07 TONES";
pub const EFFECTS_LABEL: &str = "08 EFFECTS";

/// ユーザー色の固定パレット（Issue #58、ADR-0013）。(タグ, 面, 縁)。
/// テーマの2色相（ピンク系・ティール系）と暖色の中間色から作る淡色で
/// 統一し、再生中専用色 ACCENT そのものは面にも縁にも含めない。
/// Pink の面と Mint の面・縁はデザイン文書の色（ACCENT_SOFT / MINT /
/// TEAL）そのもの。
pub const PALETTE: &[(studio_core::ColorTag, Rgb, Rgb)] = &[
    // Pink #FFC7DE / 縁 #E56BA3
    (
        studio_core::ColorTag::Pink,
        (1.000, 0.780, 0.871),
        (0.898, 0.420, 0.639),
    ),
    // Coral #FFB59E / 縁 #E07856
    (
        studio_core::ColorTag::Coral,
        (1.000, 0.710, 0.620),
        (0.878, 0.471, 0.337),
    ),
    // Peach #FFD9B8 / 縁 #DE9A5F
    (
        studio_core::ColorTag::Peach,
        (1.000, 0.851, 0.722),
        (0.871, 0.604, 0.373),
    ),
    // Lemon #F7E8A0 / 縁 #C9A93B
    (
        studio_core::ColorTag::Lemon,
        (0.969, 0.910, 0.627),
        (0.788, 0.663, 0.231),
    ),
    // Mint #C4EDE6 / 縁 #12B5A5
    (
        studio_core::ColorTag::Mint,
        (0.769, 0.929, 0.902),
        (0.071, 0.710, 0.647),
    ),
    // Teal #12B5A5 / 縁 #0B7A6E
    (
        studio_core::ColorTag::Teal,
        (0.071, 0.710, 0.647),
        (0.043, 0.478, 0.431),
    ),
    // Sky #BFDCE8 / 縁 #5E93AC
    (
        studio_core::ColorTag::Sky,
        (0.749, 0.863, 0.910),
        (0.369, 0.576, 0.675),
    ),
    // Lavender #D9CFF0 / 縁 #8B76C4
    (
        studio_core::ColorTag::Lavender,
        (0.851, 0.812, 0.941),
        (0.545, 0.463, 0.769),
    ),
];

/// パレット識別子から（面, 縁）の RGB を引く。
pub fn palette_colors(tag: studio_core::ColorTag) -> (Rgb, Rgb) {
    PALETTE
        .iter()
        .find(|(t, _, _)| *t == tag)
        .map(|(_, face, edge)| (*face, *edge))
        .expect("PALETTE covers all ColorTag variants")
}

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
        assert_eq!(COLOR_LABEL, "05 COLOR");
        assert_eq!(PIANO_ROLL_LABEL, "06 PIANO ROLL");
        assert_eq!(TONES_LABEL, "07 TONES");
        assert_eq!(EFFECTS_LABEL, "08 EFFECTS");
    }
}

#[cfg(test)]
mod palette_tests {
    use super::*;
    use studio_core::ColorTag;

    #[test]
    fn palette_has_all_tags_and_excludes_accent() {
        // ADR-0013: パレットは全タグを含み、再生中専用色 ACCENT そのものは
        // 面にも縁にも含めない（判読性のため）。
        let tags = [
            ColorTag::Pink,
            ColorTag::Coral,
            ColorTag::Peach,
            ColorTag::Lemon,
            ColorTag::Mint,
            ColorTag::Teal,
            ColorTag::Sky,
            ColorTag::Lavender,
        ];
        assert_eq!(PALETTE.len(), tags.len());
        for tag in tags {
            let (face, edge) = palette_colors(tag);
            assert_ne!(face, ACCENT, "{tag:?} の面が ACCENT と同じ");
            assert_ne!(edge, ACCENT, "{tag:?} の縁が ACCENT と同じ");
        }
        // 文書化した2色はデザイン文書の色と一致する
        assert_eq!(palette_colors(ColorTag::Pink).0, ACCENT_SOFT);
        assert_eq!(palette_colors(ColorTag::Mint), (MINT, TEAL));
    }
}
