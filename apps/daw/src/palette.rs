//! 色選択パネル（Issue #58、ADR-0013）のレイアウトと当たり判定。
//!
//! GUI（nannou）に依存しない純粋関数として置き、単体テストで固める。
//! パネルはトランスポートの下に置き、「TRACK」行は選択中トラック、
//! 「CLIP」行は選択中トラックの再生位置にあるクリップを対象とする
//! （対象の決定は main.rs 側。ここは座標→（対象, 色）の変換だけ）。
//! 各行の先頭は「なし」（未設定に戻す）のスウォッチ。

use studio_core::ColorTag;

use crate::{mixer, theme};

/// スウォッチの一辺（px）。
pub const SWATCH_PX: f32 = 20.0;
/// スウォッチ間の余白（px）。
pub const SWATCH_GAP_PX: f32 = 6.0;
/// 行ラベル（TRACK / CLIP）の幅（px）。
pub const ROW_LABEL_W_PX: f32 = 52.0;
/// 1行の高さ（px）。
pub const ROW_H_PX: f32 = 26.0;
/// スウォッチ列の開始 x（px）。
pub const SWATCH_X_PX: f32 = 72.0;
/// パネル上端から1行目までの余白（ラベル分、px）。
pub const ROWS_TOP_OFFSET_PX: f32 = 16.0;

/// 色の適用対象。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteTarget {
    Track,
    Clip,
}

/// パネルの上端 y。トランスポートパネル（上端 +28 基準、高さ 64）の
/// 下に 12px 空けて置く。
pub fn panel_top(track_count: usize) -> f32 {
    mixer::mixer_top_y(track_count)
        + track_count as f32 * mixer::STRIP_HEIGHT_PX
        + 28.0
        + 56.0
        + 12.0
}

/// パネルの高さ（ラベル＋2行）。
pub fn panel_height() -> f32 {
    ROWS_TOP_OFFSET_PX + ROW_H_PX * 2.0 + 4.0
}

fn row_top(panel_top_y: f32, target: PaletteTarget) -> f32 {
    panel_top_y
        + ROWS_TOP_OFFSET_PX
        + match target {
            PaletteTarget::Track => 0.0,
            PaletteTarget::Clip => ROW_H_PX,
        }
}

/// スウォッチの矩形。`index` 0 は「なし」、1.. は theme::PALETTE の順。
pub fn swatch_rect(panel_top_y: f32, target: PaletteTarget, index: usize) -> mixer::Rect {
    mixer::Rect {
        x: SWATCH_X_PX + index as f32 * (SWATCH_PX + SWATCH_GAP_PX),
        y: row_top(panel_top_y, target) + (ROW_H_PX - SWATCH_PX) / 2.0,
        w: SWATCH_PX,
        h: SWATCH_PX,
    }
}

/// パネルへのヒットテスト。当たれば（対象, 色）を返す。
/// 色 None は「なし」スウォッチ（未設定に戻す）。
pub fn hit_palette(
    track_count: usize,
    x: f32,
    y: f32,
) -> Option<(PaletteTarget, Option<ColorTag>)> {
    let top = panel_top(track_count);
    for target in [PaletteTarget::Track, PaletteTarget::Clip] {
        // 先頭は「なし」
        if swatch_rect(top, target, 0).contains(x, y) {
            return Some((target, None));
        }
        for (i, (tag, _, _)) in theme::PALETTE.iter().enumerate() {
            if swatch_rect(top, target, i + 1).contains(x, y) {
                return Some((target, Some(*tag)));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_palette_maps_swatches() {
        // 1トラック時のパネルで、TRACK 行の「なし」と Mint、
        // CLIP 行の Sky が引けること。
        let top = panel_top(1);
        let none = swatch_rect(top, PaletteTarget::Track, 0);
        assert_eq!(
            hit_palette(1, none.x + 10.0, none.y + 10.0),
            Some((PaletteTarget::Track, None))
        );
        // PALETTE の順: Pink, Coral, Peach, Lemon, Mint(5番目=index 5)
        let mint = swatch_rect(top, PaletteTarget::Track, 5);
        assert_eq!(
            hit_palette(1, mint.x + 10.0, mint.y + 10.0),
            Some((PaletteTarget::Track, Some(ColorTag::Mint)))
        );
        // Sky は7番目（index 7）
        let sky = swatch_rect(top, PaletteTarget::Clip, 7);
        assert_eq!(
            hit_palette(1, sky.x + 10.0, sky.y + 10.0),
            Some((PaletteTarget::Clip, Some(ColorTag::Sky)))
        );
        // ラベル領域とパネル外は対象外
        assert_eq!(hit_palette(1, 30.0, none.y + 10.0), None);
        assert_eq!(hit_palette(1, none.x + 10.0, top - 20.0), None);
    }

    #[test]
    fn panel_sits_below_transport() {
        // トランスポートパネル（mixer の下 +28 から 64 高）と重ならない。
        let transport_bottom = mixer::mixer_top_y(1) + mixer::STRIP_HEIGHT_PX + 28.0 + 56.0;
        assert!(panel_top(1) >= transport_bottom);
    }
}
