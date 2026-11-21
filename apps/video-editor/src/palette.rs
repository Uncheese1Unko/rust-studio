//! 色選択パネル（Issue #59、ADR-0013）のレイアウトと当たり判定。
//!
//! GUI（nannou）に依存しない純粋関数として置き、単体テストで固める。
//! DAW #58 の palette と同じ操作系で、パネルはトラック追加エリアの
//! 下に置く。「TRACK」行は選択中トラック、「CLIP」行は選択中
//! トラックの再生位置にあるクリップを対象とする（対象の決定は
//! main.rs 側。ここは座標→（対象, 色）の変換だけ）。
//! 各行の先頭は「なし」（未設定に戻す）のスウォッチ。

use studio_core::ColorTag;

use crate::timeline;

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
/// パネルとトラック追加エリアの間隔（px）。
pub const PANEL_GAP_PX: f32 = 12.0;

/// 色の適用対象。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteTarget {
    Track,
    Clip,
}

/// 左上原点の矩形（当たり判定用）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// パネルの上端 y。トラック追加エリア（最終行直下の1行分）の下。
pub fn panel_top(track_count: usize) -> f32 {
    timeline::RULER_HEIGHT_PX + (track_count + 1) as f32 * timeline::TRACK_HEIGHT_PX + PANEL_GAP_PX
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

/// スウォッチの矩形。`index` 0 は「なし」、1.. は timeline::PALETTE の順。
pub fn swatch_rect(panel_top_y: f32, target: PaletteTarget, index: usize) -> Rect {
    Rect {
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
        if swatch_rect(top, target, 0).contains(x, y) {
            return Some((target, None));
        }
        for (i, (tag, _)) in timeline::PALETTE.iter().enumerate() {
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
        // 3トラック時のパネルで、TRACK 行の「なし」と Mint、
        // CLIP 行の Sky が引けること。
        let top = panel_top(3);
        let none = swatch_rect(top, PaletteTarget::Track, 0);
        assert_eq!(
            hit_palette(3, none.x + 10.0, none.y + 10.0),
            Some((PaletteTarget::Track, None))
        );
        // PALETTE の順: Pink, Coral, Peach, Lemon, Mint(5番目=index 5)
        let mint = swatch_rect(top, PaletteTarget::Track, 5);
        assert_eq!(
            hit_palette(3, mint.x + 10.0, mint.y + 10.0),
            Some((PaletteTarget::Track, Some(ColorTag::Mint)))
        );
        // Sky は7番目（index 7）
        let sky = swatch_rect(top, PaletteTarget::Clip, 7);
        assert_eq!(
            hit_palette(3, sky.x + 10.0, sky.y + 10.0),
            Some((PaletteTarget::Clip, Some(ColorTag::Sky)))
        );
        // ラベル領域とパネル外は対象外
        assert_eq!(hit_palette(3, 30.0, none.y + 10.0), None);
        assert_eq!(hit_palette(3, none.x + 10.0, top - 20.0), None);
    }

    #[test]
    fn panel_sits_below_track_add_area() {
        // 追加エリア（最終行直下の1行）と重ならない。
        let add_bottom = timeline::RULER_HEIGHT_PX + 4.0 * timeline::TRACK_HEIGHT_PX;
        assert!(panel_top(3) >= add_bottom);
    }
}
