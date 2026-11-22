//! ピアノロール（Issue #69、ADR-0016）: ノート編集面のレイアウト
//! 計算とヒットテストを GUI 非依存に切り出したモジュール
//! （timeline モジュールと同じ方針）。座標は左上原点ピクセル。
//!
//! ロールはルーラーの下の全域を使う画面モードで、左に鍵盤列、
//! 右にグリッド（横 = クリップ相対の時刻、縦 = 音高）を置く。
//! 表示する音高は 25 行（2オクターブ＋1）で固定し、開いたときに
//! クリップのノートが収まるよう中心を決める（スクロールはしない）。

use studio_core::{Clip, Command, Note};

/// 表示する音高の行数。
pub const ROWS: i32 = 25;
/// 左の鍵盤列の幅（px）。
pub const KEY_WIDTH_PX: f32 = 46.0;
/// グリッドの上の見出し帯の高さ（px）。
pub const HEADER_HEIGHT_PX: f32 = 26.0;
/// 下端の余白（px）。
pub const BOTTOM_MARGIN_PX: f32 = 8.0;
/// 見出し帯のパネルラベルの中心 x（px、Issue #74）。
pub const HEADER_LABEL_CENTER_X: f32 = 52.0;
/// 見出し帯のパネルラベルの半幅（px、"06 PIANO ROLL" を 11px で描く想定）。
pub const HEADER_LABEL_HALF_W: f32 = 48.0;
/// 見出し帯の操作ヒントの中心（右端からの px、Issue #74）。
pub const HEADER_HINT_CENTER_FROM_RIGHT: f32 = 190.0;
/// 見出し帯の操作ヒントの半幅（px）。
pub const HEADER_HINT_HALF_W: f32 = 160.0;

/// 見出し帯のタイトル（クリップ名＋楽器）の中心 x（Issue #74）。
/// タイトルはラベルとヒントの間の帯の中央に置く。以前は x=120 に
/// left_justify で置いていたが、nannou の left_justify はテキスト
/// ブロックの左端アンカーとしては働かず、タイトルが左へ広がって
/// ラベルに重なっていた（実測で確認）。
pub fn header_title_center_x(win_w: f32) -> f32 {
    let left = HEADER_LABEL_CENTER_X + HEADER_LABEL_HALF_W + 12.0;
    let right = win_w - HEADER_HINT_CENTER_FROM_RIGHT - HEADER_HINT_HALF_W - 12.0;
    if right <= left {
        return left;
    }
    (left + right) / 2.0
}
/// 配置するノートの既定の長さ（ms）。
pub const DEFAULT_NOTE_MS: i64 = 500;
/// 配置するノートの既定のベロシティ。
pub const DEFAULT_VELOCITY: u8 = 100;

/// グリッド領域（左上原点ピクセル）。鍵盤列は含まない。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl GridRect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// ピアノロールの表示状態（プロジェクトには保存しない GUI 状態）。
#[derive(Debug, Clone)]
pub struct PianoRoll {
    /// 編集中のクリップ。
    pub clip_id: u64,
    /// 最上行の音高（MIDI ノート番号）。
    pub top_pitch: i32,
    /// 選択中のノート（クリップ内のインデックス）。
    pub selected: Option<usize>,
    /// ドラッグ中のノート移動。
    pub drag: Option<NoteDrag>,
}

/// ノートのドラッグ移動（掴んだ位置と元の値を保持し、ポインタの
/// 移動量でコマンドを作る。タイムラインの Drag と同じ考え方）。
#[derive(Debug, Clone)]
pub struct NoteDrag {
    pub note_index: usize,
    pub grab_pitch: i32,
    pub grab_ms: i64,
    pub orig_pitch: i32,
    pub orig_start_ms: i64,
    pub note_duration_ms: i64,
}

/// グリッド領域。ルーラーの下、見出し帯の下から画面下端まで。
pub fn grid(win_w: f32, win_h: f32) -> GridRect {
    let y = crate::timeline::RULER_HEIGHT_PX + HEADER_HEIGHT_PX;
    GridRect {
        x: KEY_WIDTH_PX,
        y,
        w: (win_w - KEY_WIDTH_PX).max(0.0),
        h: (win_h - y - BOTTOM_MARGIN_PX).max(0.0),
    }
}

/// 音高名（60 = C4）。
pub fn pitch_name(pitch: i32) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[pitch.rem_euclid(12) as usize],
        pitch.div_euclid(12) - 1
    )
}

/// 黒鍵かどうか。
pub fn is_black_key(pitch: i32) -> bool {
    matches!(pitch.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

impl PianoRoll {
    /// クリップを開く。表示範囲はノート列の中心に寄せる
    /// （ノートが無ければ C4 = 60 を中心にする）。ノート列が
    /// 表示範囲（25行）より広い場合は全ては収まらず、中心付近が
    /// 見える（範囲外のノートは編集できない。スクロールは将来課題）。
    pub fn open(clip: &Clip) -> PianoRoll {
        let center = if clip.notes.is_empty() {
            60
        } else {
            let min = clip
                .notes
                .iter()
                .map(|n| n.pitch as i32)
                .min()
                .unwrap_or(60);
            let max = clip
                .notes
                .iter()
                .map(|n| n.pitch as i32)
                .max()
                .unwrap_or(60);
            (min + max) / 2
        };
        PianoRoll {
            clip_id: clip.id,
            top_pitch: (center + ROWS / 2).clamp(ROWS - 1, 127),
            selected: None,
            drag: None,
        }
    }

    /// 行の高さ（px）。
    pub fn row_h(&self, win_h: f32) -> f32 {
        grid(0.0, win_h).h / ROWS as f32
    }

    /// 音高の行の上端 y。表示範囲外でも計算は返す。
    pub fn y_for_pitch(&self, win_h: f32, pitch: i32) -> f32 {
        grid(0.0, win_h).y + (self.top_pitch - pitch) as f32 * self.row_h(win_h)
    }

    /// y から音高を引く。グリッドの外は None。
    pub fn pitch_at_y(&self, win_w: f32, win_h: f32, x: f32, y: f32) -> Option<i32> {
        let g = grid(win_w, win_h);
        if !g.contains(x, y) {
            return None;
        }
        let row = ((y - g.y) / self.row_h(win_h)).floor() as i32;
        Some(self.top_pitch - row.clamp(0, ROWS - 1))
    }

    /// y から音高を引く（範囲外は端にクランプ。ドラッグ用）。
    pub fn pitch_at_y_clamped(&self, win_h: f32, y: f32) -> i32 {
        let g = grid(0.0, win_h);
        let row = ((y - g.y) / self.row_h(win_h)).floor() as i32;
        self.top_pitch - row.clamp(0, ROWS - 1)
    }

    /// クリップ相対 ms の x。
    pub fn x_for_ms(&self, win_w: f32, clip_duration_ms: i64, ms: i64) -> f32 {
        let g = grid(win_w, 0.0);
        g.x + ms as f32 / clip_duration_ms.max(1) as f32 * g.w
    }

    /// x からクリップ相対 ms を引く。グリッドの外は None。
    pub fn ms_at_x(
        &self,
        win_w: f32,
        win_h: f32,
        clip_duration_ms: i64,
        x: f32,
        y: f32,
    ) -> Option<i64> {
        if !grid(win_w, win_h).contains(x, y) {
            return None;
        }
        Some(self.ms_at_x_clamped(win_w, clip_duration_ms, x))
    }

    /// x からクリップ相対 ms を引く（範囲外は端にクランプ。ドラッグ用）。
    pub fn ms_at_x_clamped(&self, win_w: f32, clip_duration_ms: i64, x: f32) -> i64 {
        let g = grid(win_w, 0.0);
        if g.w <= 0.0 {
            return 0;
        }
        let ratio = ((x - g.x) / g.w).clamp(0.0, 1.0);
        ((ratio * clip_duration_ms.max(0) as f32).round() as i64).clamp(0, clip_duration_ms.max(0))
    }

    /// ノートの描画矩形。
    pub fn note_rect(&self, win_w: f32, win_h: f32, clip: &Clip, note: &Note) -> GridRect {
        let g = grid(win_w, win_h);
        let x = self.x_for_ms(win_w, clip.duration_ms, note.start_ms);
        let w = (note.duration_ms as f32 / clip.duration_ms.max(1) as f32 * g.w).max(1.0);
        GridRect {
            x,
            y: self.y_for_pitch(win_h, note.pitch as i32),
            w,
            h: self.row_h(win_h),
        }
    }

    /// ノートのヒットテスト。重なりは後ろ（大きいインデックス＝
    /// 上に描かれる方）を優先する。
    pub fn hit_note(&self, win_w: f32, win_h: f32, clip: &Clip, x: f32, y: f32) -> Option<usize> {
        clip.notes
            .iter()
            .enumerate()
            .rev()
            .find(|(_, note)| self.note_rect(win_w, win_h, clip, note).contains(x, y))
            .map(|(index, _)| index)
    }

    /// 空きグリッドのクリックから配置コマンドを作る。既存ノートの
    /// 上やグリッドの外では None。
    pub fn place_command(
        &self,
        win_w: f32,
        win_h: f32,
        clip: &Clip,
        x: f32,
        y: f32,
    ) -> Option<Command> {
        if self.hit_note(win_w, win_h, clip, x, y).is_some() {
            return None;
        }
        let pitch = self.pitch_at_y(win_w, win_h, x, y)?;
        let ms = self.ms_at_x(win_w, win_h, clip.duration_ms, x, y)?;
        let len = DEFAULT_NOTE_MS.min(clip.duration_ms);
        if len <= 0 {
            return None;
        }
        Some(Command::AddNote {
            clip_id: clip.id,
            pitch: pitch as u8,
            start_ms: ms.min(clip.duration_ms - len),
            duration_ms: len,
            velocity: DEFAULT_VELOCITY,
        })
    }

    /// ノートを掴んでドラッグを開始する。
    pub fn begin_drag(
        &self,
        win_w: f32,
        win_h: f32,
        clip: &Clip,
        note_index: usize,
        x: f32,
        y: f32,
    ) -> Option<NoteDrag> {
        let note = clip.notes.get(note_index)?;
        Some(NoteDrag {
            note_index,
            grab_pitch: self.pitch_at_y_clamped(win_h, y),
            grab_ms: self.ms_at_x_clamped(win_w, clip.duration_ms, x),
            orig_pitch: note.pitch as i32,
            orig_start_ms: note.start_ms,
            note_duration_ms: note.duration_ms,
        })
    }
}

impl NoteDrag {
    /// 現在のポインタ位置に対する移動コマンド。音高は表示範囲と
    /// MIDI の範囲に、開始はノートがクリップに収まる範囲に
    /// クランプする。
    pub fn command_for(
        &self,
        roll: &PianoRoll,
        win_w: f32,
        win_h: f32,
        clip: &Clip,
        x: f32,
        y: f32,
    ) -> Command {
        let cur_pitch = roll.pitch_at_y_clamped(win_h, y);
        let cur_ms = roll.ms_at_x_clamped(win_w, clip.duration_ms, x);
        let new_pitch = (self.orig_pitch + (cur_pitch - self.grab_pitch))
            .clamp(roll.top_pitch - ROWS + 1, roll.top_pitch)
            .clamp(0, 127);
        let max_start = (clip.duration_ms - self.note_duration_ms).max(0);
        let new_start = (self.orig_start_ms + (cur_ms - self.grab_ms)).clamp(0, max_start);
        Command::MoveNote {
            clip_id: clip.id,
            note_index: self.note_index,
            new_pitch: new_pitch as u8,
            new_start_ms: new_start,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_title_band_avoids_label_and_hint() {
        // Issue #74: タイトル（最大 400px 幅を想定）を帯の中央に
        // 置いても、ラベル域とヒント域に重ならない。
        let cx = header_title_center_x(960.0);
        let label_right = HEADER_LABEL_CENTER_X + HEADER_LABEL_HALF_W;
        let hint_left = 960.0 - HEADER_HINT_CENTER_FROM_RIGHT - HEADER_HINT_HALF_W;
        assert!(
            cx - 200.0 >= label_right + 12.0,
            "title left edge {} vs label right {label_right}",
            cx - 200.0
        );
        assert!(
            cx + 200.0 <= hint_left - 12.0,
            "title right edge {} vs hint left {hint_left}",
            cx + 200.0
        );
        // 狭い窓でもラベルの右から始まる。
        assert!(header_title_center_x(500.0) >= label_right + 12.0);
    }

    pub const W: f32 = 960.0;
    pub const H: f32 = 540.0;

    pub fn clip_with_notes(notes: Vec<Note>) -> Clip {
        Clip {
            id: 2,
            name: "melody".into(),
            start_ms: 0,
            duration_ms: 4_000,
            source: String::new(),
            offset_ms: 0,
            gain: 1.0,
            title_text: String::new(),
            transition_in_kind: studio_core::TransitionKind::None,
            transition_in_ms: 0,
            cutout: false,
            pitch_semitones: 0.0,
            fade_in_ms: 0,
            fade_out_ms: 0,
            color: None,
            notes,
        }
    }

    pub fn note(pitch: u8, start_ms: i64, duration_ms: i64) -> Note {
        Note {
            pitch,
            start_ms,
            duration_ms,
            velocity: 100,
        }
    }

    #[test]
    fn open_centers_on_the_notes_and_defaults_to_c4() {
        // Issue #69: 開いたときにノートが表示範囲（25行）に収まる。
        let roll = PianoRoll::open(&clip_with_notes(vec![note(55, 0, 500), note(67, 500, 500)]));
        assert!(roll.top_pitch - (ROWS - 1) <= 55, "lowest note visible");
        assert!(roll.top_pitch >= 67, "highest note visible");
        let empty = PianoRoll::open(&clip_with_notes(vec![]));
        assert!(empty.top_pitch - (ROWS - 1) <= 60 && empty.top_pitch >= 60);
    }

    #[test]
    fn pitch_and_y_round_trip() {
        // Issue #69: 音高↔座標の変換（行の中心で往復が一致する）。
        let roll = PianoRoll::open(&clip_with_notes(vec![]));
        for pitch in (roll.top_pitch - ROWS + 1)..=roll.top_pitch {
            let y = roll.y_for_pitch(H, pitch) + roll.row_h(H) / 2.0;
            let x = grid(W, H).x + 10.0;
            assert_eq!(roll.pitch_at_y(W, H, x, y), Some(pitch), "pitch {pitch}");
        }
        // グリッドの外は None
        assert_eq!(roll.pitch_at_y(W, H, 10.0, 300.0), None, "key column");
        assert_eq!(roll.pitch_at_y(W, H, 500.0, 10.0), None, "ruler area");
    }

    #[test]
    fn ms_and_x_round_trip() {
        // Issue #69: 時刻↔座標の変換（クリップの尺をグリッド幅に写す）。
        let roll = PianoRoll::open(&clip_with_notes(vec![]));
        let g = grid(W, H);
        assert_eq!(roll.ms_at_x(W, H, 4_000, g.x, 300.0), Some(0));
        let mid = roll.x_for_ms(W, 4_000, 2_000);
        assert_eq!(roll.ms_at_x(W, H, 4_000, mid, 300.0), Some(2_000));
        let end = roll.x_for_ms(W, 4_000, 4_000);
        assert!((end - (g.x + g.w)).abs() < 1.5, "end at grid right edge");
        assert_eq!(roll.ms_at_x(W, H, 4_000, 10.0, 300.0), None, "key column");
    }

    #[test]
    fn hit_note_prefers_the_topmost_and_misses_outside() {
        // Issue #69: ヒットテスト。重なりは大きいインデックス優先。
        let clip = clip_with_notes(vec![note(60, 0, 1_000), note(60, 500, 1_000)]);
        let roll = PianoRoll::open(&clip);
        let rect0 = roll.note_rect(W, H, &clip, &clip.notes[0]);
        let cx = rect0.x + 5.0;
        let cy = rect0.y + rect0.h / 2.0;
        assert_eq!(roll.hit_note(W, H, &clip, cx, cy), Some(0));
        // 重なり部分（750ms 付近）はノート1が上
        let overlap_x = roll.x_for_ms(W, clip.duration_ms, 750);
        assert_eq!(roll.hit_note(W, H, &clip, overlap_x, cy), Some(1));
        // ノートの無い場所は None
        let empty_x = roll.x_for_ms(W, clip.duration_ms, 3_000);
        assert_eq!(roll.hit_note(W, H, &clip, empty_x, cy), None);
        assert_eq!(roll.hit_note(W, H, &clip, 10.0, cy), None);
    }

    #[test]
    fn place_command_builds_add_note_and_clamps_to_the_clip_end() {
        // Issue #69: 空きグリッドのクリックで AddNote。右端では
        // ノートが収まるよう開始を切り詰める。既存ノートの上は None。
        let clip = clip_with_notes(vec![note(60, 0, 500)]);
        let roll = PianoRoll::open(&clip);
        let y = roll.y_for_pitch(H, 62) + roll.row_h(H) / 2.0;
        let x = roll.x_for_ms(W, clip.duration_ms, 1_000);
        assert_eq!(
            roll.place_command(W, H, &clip, x, y),
            Some(Command::AddNote {
                clip_id: 2,
                pitch: 62,
                start_ms: 1_000,
                duration_ms: DEFAULT_NOTE_MS,
                velocity: DEFAULT_VELOCITY,
            })
        );
        // 右端近く: 開始は duration - DEFAULT_NOTE_MS にクランプ
        let x_end = roll.x_for_ms(W, clip.duration_ms, 3_900);
        match roll.place_command(W, H, &clip, x_end, y) {
            Some(Command::AddNote { start_ms, .. }) => {
                assert_eq!(start_ms, 4_000 - DEFAULT_NOTE_MS)
            }
            other => panic!("expected clamped AddNote, got {other:?}"),
        }
        // 既存ノートの上は配置しない
        let rect = roll.note_rect(W, H, &clip, &clip.notes[0]);
        assert_eq!(
            roll.place_command(W, H, &clip, rect.x + 5.0, rect.y + 2.0),
            None
        );
        // グリッドの外は配置しない
        assert_eq!(roll.place_command(W, H, &clip, 10.0, y), None);
    }

    #[test]
    fn drag_moves_by_the_pointer_delta_and_clamps() {
        // Issue #69: ドラッグは掴んだ位置からの移動量で MoveNote を
        // 作り、クリップの端と表示範囲でクランプする。
        let clip = clip_with_notes(vec![note(60, 1_000, 500)]);
        let roll = PianoRoll::open(&clip);
        let rect = roll.note_rect(W, H, &clip, &clip.notes[0]);
        let gx = rect.x + 10.0;
        let gy = rect.y + rect.h / 2.0;
        let drag = roll.begin_drag(W, H, &clip, 0, gx, gy).unwrap();
        // 右へ500ms、上へ2半音
        let x2 = roll.x_for_ms(W, clip.duration_ms, 1_500) + 10.0;
        let y2 = roll.y_for_pitch(H, 62) + roll.row_h(H) / 2.0;
        assert_eq!(
            drag.command_for(&roll, W, H, &clip, x2, y2),
            Command::MoveNote {
                clip_id: 2,
                note_index: 0,
                new_pitch: 62,
                new_start_ms: 1_500,
            }
        );
        // 左端より前へは行かない
        let cmd = drag.command_for(&roll, W, H, &clip, grid(W, H).x - 500.0, gy);
        match cmd {
            Command::MoveNote { new_start_ms, .. } => assert_eq!(new_start_ms, 0),
            other => panic!("{other:?}"),
        }
        // 右端は duration - note duration で止まる
        let cmd = drag.command_for(&roll, W, H, &clip, W + 500.0, gy);
        match cmd {
            Command::MoveNote { new_start_ms, .. } => {
                assert_eq!(new_start_ms, 4_000 - 500)
            }
            other => panic!("{other:?}"),
        }
        // 音高は表示範囲の端で止まる
        let cmd = drag.command_for(&roll, W, H, &clip, gx, -500.0);
        match cmd {
            Command::MoveNote { new_pitch, .. } => {
                assert_eq!(new_pitch, roll.top_pitch as u8)
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn pitch_names_and_black_keys() {
        assert_eq!(pitch_name(60), "C4");
        assert_eq!(pitch_name(61), "C#4");
        assert_eq!(pitch_name(69), "A4");
        assert_eq!(pitch_name(59), "B3");
        assert!(is_black_key(61));
        assert!(!is_black_key(60));
    }
}

#[cfg(test)]
mod review_tests {
    use super::tests::*;
    use super::*;

    #[test]
    fn drag_clamps_pitch_at_the_bottom_row() {
        // レビュー指摘（Issue #69）: 音高の下端クランプも固定する。
        let clip = clip_with_notes(vec![note(60, 1_000, 500)]);
        let roll = PianoRoll::open(&clip);
        let rect = roll.note_rect(W, H, &clip, &clip.notes[0]);
        let drag = roll
            .begin_drag(W, H, &clip, 0, rect.x + 10.0, rect.y + rect.h / 2.0)
            .unwrap();
        let cmd = drag.command_for(&roll, W, H, &clip, rect.x + 10.0, H + 500.0);
        match cmd {
            Command::MoveNote { new_pitch, .. } => {
                assert_eq!(new_pitch, (roll.top_pitch - ROWS + 1) as u8)
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn open_centers_even_when_notes_span_more_than_the_rows() {
        // レビュー指摘（Issue #69）: 25行より広いノート列では
        // 全ては収まらない。中心に寄せることを固定する。
        let clip = clip_with_notes(vec![note(40, 0, 500), note(80, 500, 500)]);
        let roll = PianoRoll::open(&clip);
        assert_eq!(roll.top_pitch, 60 + ROWS / 2);
    }
}
