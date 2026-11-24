//! 音色パネル（Issue #73、ADR-0018）のレイアウトと当たり判定。
//!
//! GUI（nannou）に依存しない純粋関数として置き、単体テストで固める。
//! パネルは色選択パネルの下に置く。行には音色チップが名前順ではなく
//! 作成順に並び、末尾に「作成（選択中トラックの楽器から）」と
//! 「削除（選択中トラックに割り当て中の音色）」のボタンを置く。
//! 対象トラックの決定とコマンド適用は main.rs 側の責務で、ここは
//! 座標→操作の変換だけを持つ。
//!
//! Issue #76: チップとボタンはフロー配置で、右端（`limit_x`、
//! 呼び出し側がウィンドウ幅から決める）を超えるアイテムは次の行の
//! 左端へ折り返す。パネルの高さは行数に応じて伸びる。あわせて、
//! チップの右クリックでその音色を削除でき（割り当て中かどうかを
//! 問わない。適用は main.rs 側）、ダブルクリックで改名セッション
//! （`ToneRenameSession`）に入れる。

use studio_core::{Command, Tone};

use crate::edit::{RenameAction, RenameInput};
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

/// パネルの高さ（ラベル＋行数分、Issue #76）。
pub fn panel_height(tones: &[Tone], limit_x: f32) -> f32 {
    ROWS_TOP_OFFSET_PX + row_count(tones, limit_x) as f32 * ROW_H_PX + 4.0
}

/// チップ幅は名前の表示長で決める（最小 56、最大 150px）。
pub fn chip_width(name: &str) -> f32 {
    (name.chars().count() as f32 * 7.0 + 18.0).clamp(56.0, 150.0)
}

fn row_y(panel_top_y: f32, row: usize) -> f32 {
    panel_top_y + ROWS_TOP_OFFSET_PX + row as f32 * ROW_H_PX + (ROW_H_PX - CHIP_H_PX) / 2.0
}

/// フロー配置のアイテム（チップ全部→作成→削除の順）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlowItem {
    Chip(usize),
    Add,
    Delete,
}

fn flow_items(tones: &[Tone]) -> Vec<FlowItem> {
    let mut items: Vec<FlowItem> = (0..tones.len()).map(FlowItem::Chip).collect();
    items.push(FlowItem::Add);
    items.push(FlowItem::Delete);
    items
}

fn item_width(tones: &[Tone], item: FlowItem) -> f32 {
    match item {
        FlowItem::Chip(index) => chip_width(&tones[index].name),
        FlowItem::Add => ADD_W_PX,
        FlowItem::Delete => DELETE_W_PX,
    }
}

/// フロー配置を1回走査し、(アイテム, x, 行) を順に返す。
/// 右端を超えるアイテムは次の行の左端へ回す。行頭のアイテムは
/// 幅が残り幅を超えてもその行に置く（置き場が無いため）。
fn flow_layout(tones: &[Tone], limit_x: f32) -> Vec<(FlowItem, f32, usize)> {
    let mut placed = Vec::new();
    let mut x = CHIP_X_PX;
    let mut row = 0usize;
    for item in flow_items(tones) {
        let w = item_width(tones, item);
        if x > CHIP_X_PX && x + w > limit_x {
            row += 1;
            x = CHIP_X_PX;
        }
        placed.push((item, x, row));
        x += w + CHIP_GAP_PX;
    }
    placed
}

/// 折り返し後の行数。
fn row_count(tones: &[Tone], limit_x: f32) -> usize {
    flow_layout(tones, limit_x)
        .iter()
        .map(|(_, _, row)| *row)
        .max()
        .unwrap_or(0)
        + 1
}

fn flow_rect(panel_top_y: f32, tones: &[Tone], target: FlowItem, limit_x: f32) -> mixer::Rect {
    for (item, x, row) in flow_layout(tones, limit_x) {
        if item == target {
            return mixer::Rect {
                x,
                y: row_y(panel_top_y, row),
                w: item_width(tones, item),
                h: CHIP_H_PX,
            };
        }
    }
    unreachable!("flow items always include the target");
}

/// 音色チップの矩形（作成順に左から並び、右端で折り返す）。
pub fn chip_rect(panel_top_y: f32, tones: &[Tone], index: usize, limit_x: f32) -> mixer::Rect {
    flow_rect(panel_top_y, tones, FlowItem::Chip(index), limit_x)
}

/// 作成ボタンの矩形（チップ列の末尾、フローに従う）。
pub fn add_rect(panel_top_y: f32, tones: &[Tone], limit_x: f32) -> mixer::Rect {
    flow_rect(panel_top_y, tones, FlowItem::Add, limit_x)
}

/// 削除ボタンの矩形（作成ボタンの後、フローに従う）。
pub fn delete_rect(panel_top_y: f32, tones: &[Tone], limit_x: f32) -> mixer::Rect {
    flow_rect(panel_top_y, tones, FlowItem::Delete, limit_x)
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
pub fn hit_tones(
    track_count: usize,
    tones: &[Tone],
    x: f32,
    y: f32,
    limit_x: f32,
) -> Option<ToneHit> {
    let top = panel_top(track_count);
    for index in 0..tones.len() {
        if chip_rect(top, tones, index, limit_x).contains(x, y) {
            return Some(ToneHit::Chip(index));
        }
    }
    if add_rect(top, tones, limit_x).contains(x, y) {
        return Some(ToneHit::Add);
    }
    if delete_rect(top, tones, limit_x).contains(x, y) {
        return Some(ToneHit::Delete);
    }
    None
}

/// 音色の改名セッション（Issue #76）。トラック改名
/// （`edit::RenameSession`）と同じ意味論で、確定時にだけ
/// RenameTone コマンドを返す。GUI 側の表示状態であり、
/// プロジェクト状態には含めない。
#[derive(Debug, Clone, PartialEq)]
pub struct ToneRenameSession {
    pub tone_id: u64,
    pub text: String,
}

impl ToneRenameSession {
    pub fn begin(tone_id: u64, current_name: &str) -> ToneRenameSession {
        ToneRenameSession {
            tone_id,
            text: current_name.to_string(),
        }
    }

    pub fn handle(&mut self, input: RenameInput) -> RenameAction {
        match input {
            RenameInput::Char(c) => {
                if !c.is_control() {
                    self.text.push(c);
                }
                RenameAction::None
            }
            RenameInput::Backspace => {
                self.text.pop();
                RenameAction::None
            }
            RenameInput::Enter => RenameAction::Commit(Command::RenameTone {
                tone_id: self.tone_id,
                name: self.text.clone(),
            }),
            RenameInput::Escape => RenameAction::Cancel,
        }
    }
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
        let chip0 = chip_rect(top, &tones, 0, 944.0);
        assert_eq!(
            hit_tones(2, &tones, chip0.x + 5.0, chip0.y + 5.0, 944.0),
            Some(ToneHit::Chip(0))
        );
        let chip1 = chip_rect(top, &tones, 1, 944.0);
        assert_eq!(
            hit_tones(2, &tones, chip1.x + 5.0, chip1.y + 5.0, 944.0),
            Some(ToneHit::Chip(1))
        );
        let add = add_rect(top, &tones, 944.0);
        assert_eq!(
            hit_tones(2, &tones, add.x + 5.0, add.y + 5.0, 944.0),
            Some(ToneHit::Add)
        );
        let del = delete_rect(top, &tones, 944.0);
        assert_eq!(
            hit_tones(2, &tones, del.x + 5.0, del.y + 5.0, 944.0),
            Some(ToneHit::Delete)
        );
        // ラベル領域とパネル外は対象外。
        assert_eq!(hit_tones(2, &tones, 30.0, chip0.y + 5.0, 944.0), None);
        assert_eq!(hit_tones(2, &tones, chip0.x + 5.0, top - 20.0, 944.0), None);
    }

    #[test]
    fn chips_wrap_to_next_row_at_limit() {
        // Issue #76: 音色が増えたらチップは右端（limit_x）で
        // 次の行へ折り返し、パネル／ウィンドウの外へ出ない。
        let tones: Vec<Tone> = (1..=20).map(|i| tone(i, &format!("Tone {i:02}"))).collect();
        let top = panel_top(2);
        let first = chip_rect(top, &tones, 0, 944.0);
        for index in 0..tones.len() {
            let rect = chip_rect(top, &tones, index, 944.0);
            assert!(
                rect.x + rect.w <= 944.0,
                "chip {index} overflows: x={} w={}",
                rect.x,
                rect.w
            );
        }
        // 13個目は2行目の左端に来る
        let wrapped = chip_rect(top, &tones, 12, 944.0);
        assert_eq!(wrapped.x, CHIP_X_PX);
        assert!(wrapped.y > first.y);
        // 作成／削除ボタンもフローに乗り、右端を超えない
        let add = add_rect(top, &tones, 944.0);
        let del = delete_rect(top, &tones, 944.0);
        assert!(add.x + add.w <= 944.0);
        assert!(del.x + del.w <= 944.0);
        // 折り返した先のチップもヒットする
        assert_eq!(
            hit_tones(2, &tones, wrapped.x + 5.0, wrapped.y + 5.0, 944.0),
            Some(ToneHit::Chip(12))
        );
        // パネルの高さは行数分だけ伸びる（2行 → 16 + 2*26 + 4）
        assert_eq!(panel_height(&tones, 944.0), 72.0);
        // 1行に収まる場合は従来どおり
        let few = vec![tone(1, "Pad"), tone(2, "Lead")];
        assert_eq!(panel_height(&few, 944.0), 46.0);
    }

    #[test]
    fn chips_wrap_in_narrow_width_and_oversized_chip_stays_at_row_head() {
        // Issue #76 レビュー任意指摘: 狭い幅でも折り返しが破綻
        // しないことと、行頭のアイテムは幅が残り幅を超えても
        // その行に置かれること（置き場が無いための仕様）。
        let tones: Vec<Tone> = (1..=6).map(|i| tone(i, &format!("Tone {i:02}"))).collect();
        let top = panel_top(2);
        // limit 200 では1行に1〜2個しか入らない
        let rows: Vec<f32> = (0..tones.len())
            .map(|i| chip_rect(top, &tones, i, 200.0).y)
            .collect();
        assert!(rows.windows(2).all(|w| w[1] >= w[0]));
        assert!(*rows.last().unwrap() > rows[0]);
        // 名前が最大幅（150px）のチップは、残り幅（200-72=128）を
        // 超えても行頭に置かれ、ヒットする
        let wide = vec![tone(1, &"x".repeat(100))];
        let rect = chip_rect(top, &wide, 0, 200.0);
        assert_eq!(rect.x, CHIP_X_PX);
        assert_eq!(rect.w, 150.0);
        assert_eq!(
            hit_tones(2, &wide, rect.x + 5.0, rect.y + 5.0, 200.0),
            Some(ToneHit::Chip(0))
        );
    }

    #[test]
    fn tone_rename_session_edits_and_commits() {
        // Issue #76: 音色の改名セッション。Enter で RenameTone を
        // 発行し、Esc で中止する（トラック改名と同じ意味論）。
        use crate::edit::{RenameAction, RenameInput};
        use studio_core::Command;
        let mut session = ToneRenameSession::begin(7, "Pad");
        assert_eq!(session.handle(RenameInput::Char('X')), RenameAction::None);
        assert_eq!(session.text, "PadX");
        assert_eq!(session.handle(RenameInput::Backspace), RenameAction::None);
        assert_eq!(session.text, "Pad");
        // 制御文字は取り込まない
        session.handle(RenameInput::Char('\r'));
        assert_eq!(session.text, "Pad");
        assert_eq!(
            session.handle(RenameInput::Enter),
            RenameAction::Commit(Command::RenameTone {
                tone_id: 7,
                name: "Pad".into(),
            })
        );
        let mut session = ToneRenameSession::begin(7, "Pad");
        assert_eq!(session.handle(RenameInput::Escape), RenameAction::Cancel);
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
