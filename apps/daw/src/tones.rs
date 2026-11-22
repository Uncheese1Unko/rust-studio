//! 音色パネル（Issue #73、ADR-0018）のレイアウトと当たり判定。
//!
//! GUI（nannou）に依存しない純粋関数として置き、単体テストで固める。
//! パネルは色選択パネルの下に置く。行には音色チップが名前順ではなく
//! 作成順に並び、末尾に「作成（選択中トラックの楽器から）」と
//! 「削除（選択中トラックに割り当て中の音色）」のボタンを置く。
//! 対象トラックの決定とコマンド適用は main.rs 側の責務で、ここは
//! 座標→操作の変換だけを持つ。

use studio_core::Tone;

use crate::{mixer, palette};

/// チップの高さ（px）。
pub const CHIP_H_PX: f32 = 20.0;
/// チップ間の余白（px）。
pub const CHIP_GAP_PX: f32 = 6.0;
/// チップ列の開始 x（px）。色パネルのスウォッチ列に揃える。
pub const CHIP_X_PX: f32 = 72.0;
/// パネル上端から行までの余白（ラベル分、px）。
pub const ROWS_TOP_OFFSET_PX: f32 = 16.0;
/// 行の高さ（px）。
pub const ROW_H_PX: f32 = 26.0;
/// 作成ボタンの幅（px）。
pub const ADD_W_PX: f32 = 132.0;
/// 削除ボタンの幅（px）。
pub const DELETE_W_PX: f32 = 132.0;

/// パネルの上端 y。色パネルの下に 12px 空けて置く。
pub fn panel_top(track_count: usize) -> f32 {
    palette::panel_top(track_count) + palette::panel_height() + 12.0
}

/// パネルの高さ（ラベル＋1行）。
pub fn panel_height() -> f32 {
    ROWS_TOP_OFFSET_PX + ROW_H_PX + 4.0
}

/// チップ幅は名前の表示長で決める（最小 56、最大 150px）。
pub fn chip_width(name: &str) -> f32 {
    (name.chars().count() as f32 * 7.0 + 18.0).clamp(56.0, 150.0)
}

fn row_y(panel_top_y: f32) -> f32 {
    panel_top_y + ROWS_TOP_OFFSET_PX + (ROW_H_PX - CHIP_H_PX) / 2.0
}

/// 音色チップの矩形（作成順に左から並ぶ）。
pub fn chip_rect(panel_top_y: f32, tones: &[Tone], index: usize) -> mixer::Rect {
    let mut x = CHIP_X_PX;
    for tone in &tones[..index] {
        x += chip_width(&tone.name) + CHIP_GAP_PX;
    }
    mixer::Rect {
        x,
        y: row_y(panel_top_y),
        w: chip_width(&tones[index].name),
        h: CHIP_H_PX,
    }
}

/// 作成ボタンの矩形（チップ列の末尾）。
pub fn add_rect(panel_top_y: f32, tones: &[Tone]) -> mixer::Rect {
    let mut x = CHIP_X_PX;
    for tone in tones {
        x += chip_width(&tone.name) + CHIP_GAP_PX;
    }
    mixer::Rect {
        x,
        y: row_y(panel_top_y),
        w: ADD_W_PX,
        h: CHIP_H_PX,
    }
}

/// 削除ボタンの矩形（作成ボタンの右）。
pub fn delete_rect(panel_top_y: f32, tones: &[Tone]) -> mixer::Rect {
    let add = add_rect(panel_top_y, tones);
    mixer::Rect {
        x: add.x + add.w + CHIP_GAP_PX,
        y: add.y,
        w: DELETE_W_PX,
        h: CHIP_H_PX,
    }
}

/// パネルへのヒット。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneHit {
    /// 音色チップ（tones のインデックス）。
    Chip(usize),
    /// 作成ボタン。
    Add,
    /// 削除ボタン。
    Delete,
}

/// パネルへのヒットテスト。当たれば操作を返す。
pub fn hit_tones(track_count: usize, tones: &[Tone], x: f32, y: f32) -> Option<ToneHit> {
    let top = panel_top(track_count);
    for index in 0..tones.len() {
        if chip_rect(top, tones, index).contains(x, y) {
            return Some(ToneHit::Chip(index));
        }
    }
    if add_rect(top, tones).contains(x, y) {
        return Some(ToneHit::Add);
    }
    if delete_rect(top, tones).contains(x, y) {
        return Some(ToneHit::Delete);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::Instrument;

    fn tone(id: u64, name: &str) -> Tone {
        Tone {
            id,
            name: name.into(),
            instrument: Instrument::Sampler {
                source: "a.wav".into(),
                root_note: 60,
            },
        }
    }

    #[test]
    fn hit_tones_maps_chips_and_buttons() {
        let tones = vec![tone(1, "Pad"), tone(2, "Lead")];
        let top = panel_top(2);
        let chip0 = chip_rect(top, &tones, 0);
        assert_eq!(
            hit_tones(2, &tones, chip0.x + 5.0, chip0.y + 5.0),
            Some(ToneHit::Chip(0))
        );
        let chip1 = chip_rect(top, &tones, 1);
        assert_eq!(
            hit_tones(2, &tones, chip1.x + 5.0, chip1.y + 5.0),
            Some(ToneHit::Chip(1))
        );
        let add = add_rect(top, &tones);
        assert_eq!(
            hit_tones(2, &tones, add.x + 5.0, add.y + 5.0),
            Some(ToneHit::Add)
        );
        let del = delete_rect(top, &tones);
        assert_eq!(
            hit_tones(2, &tones, del.x + 5.0, del.y + 5.0),
            Some(ToneHit::Delete)
        );
        // ラベル領域とパネル外は対象外。
        assert_eq!(hit_tones(2, &tones, 30.0, chip0.y + 5.0), None);
        assert_eq!(hit_tones(2, &tones, chip0.x + 5.0, top - 20.0), None);
    }

    #[test]
    fn panel_sits_below_palette() {
        // 色パネルと重ならない。
        assert!(panel_top(2) >= palette::panel_top(2) + palette::panel_height());
    }

    #[test]
    fn chip_width_grows_with_name_but_is_clamped() {
        assert_eq!(chip_width("A"), 56.0);
        assert!(chip_width("Warm Pad") > chip_width("Pad"));
        assert_eq!(chip_width(&"x".repeat(100)), 150.0);
    }
}
