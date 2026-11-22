//! ポインタ操作 → コマンド変換（Issue #9）。
//!
//! GUI のマウス操作を、studio-core のコマンドに変換する純粋ロジック。
//! 状態変更はここでは行わず、コマンドを返すだけ（適用はコマンド層、
//! ADR-0004）。ドラッグ中は「開始時点のクリップ状態＋ポインタの総移動量」
//! から候補コマンドを作る。範囲外の候補（負の位置など）もそのまま返し、
//! 弾くのはコマンド層の責任とする（受け入れ条件2の分担）。

use studio_core::{Command, Project, TrackKind};

use crate::timeline;

/// クリップ端とみなす掴み幅（px）。
pub const EDGE_GRAB_PX: f32 = 6.0;
/// ダブルクリックで配置するクリップの長さ（ms）。
pub const NEW_CLIP_DURATION_MS: i64 = 2_000;
/// ダブルクリックと判定する間隔（ms）と距離（px）。
pub const DOUBLE_CLICK_MS: u64 = 400;
pub const DOUBLE_CLICK_PX: f32 = 5.0;

/// Undo/Redo のキー操作（Issue #31）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryOp {
    Undo,
    Redo,
}

/// キー状態から履歴操作を決める純粋関数。
/// テキスト編集中はショートカットを奪わず、Ctrl なしの Z/Y も対象外。
/// Ctrl+Z = undo、Ctrl+Shift+Z / Ctrl+Y = redo。
pub fn resolve_history_op(
    editing: bool,
    ctrl: bool,
    shift: bool,
    z_down: bool,
    y_down: bool,
) -> Option<HistoryOp> {
    if editing || !ctrl {
        return None;
    }
    if z_down {
        Some(if shift {
            HistoryOp::Redo
        } else {
            HistoryOp::Undo
        })
    } else if y_down {
        Some(HistoryOp::Redo)
    } else {
        None
    }
}

/// 解決した履歴操作を History に適用する。実施できたら true。
pub fn apply_history_op(history: &mut studio_core::History, op: HistoryOp) -> bool {
    match op {
        HistoryOp::Undo => history.undo(),
        HistoryOp::Redo => history.redo(),
    }
}

/// ジェスチャ開始時スナップショットを、現在の状態と異なるときだけ
/// 履歴へ記録する（Issue #31）。同値ならスナップショットは保持した
/// ままで何もしない。記録されないのにスナップショットだけが消費
/// されると、継続中のジェスチャが二度と記録されなくなるため
/// （レビュー round 4 対応）。
pub fn flush_gesture_snapshot(history: &mut studio_core::History, before: &mut Option<Project>) {
    let changed = before
        .as_ref()
        .is_some_and(|snapshot| snapshot.dump_json() != history.project().dump_json());
    if changed {
        let snapshot = before.take().expect("checked above");
        history.record_undo_snapshot(snapshot);
    }
}

/// 履歴キー操作を、進行中ジェスチャへの配慮込みで適用する
/// （Issue #31 レビュー round 3・4 対応）。
///
/// - Undo: 先にジェスチャを確定させる（`flush_gesture_snapshot`）。
///   ジェスチャが変化済みなら undo はジェスチャ自身のエントリを
///   消費して開始状態へ戻り、確定済みの過去状態は失われない。
///   ジェスチャが未変化で undo が空振りしても、スナップショットは
///   保持されたままなのでジェスチャは継続できる。
/// - Redo: ジェスチャは確定させない。確定は redo スタックを消す
///   ため、その直後の redo が必ず失敗し、失敗の巻き添えで継続中
///   ジェスチャのスナップショットまで失われる。redo が成功したら
///   呼び出し側がジェスチャを打ち切り、失敗したら何も変わらない
///   （ジェスチャは継続し、終了時に通常どおり記録される）。
///
/// 実施できたら true。呼び出し側は true のときにジェスチャ状態を
/// 打ち切ること。
pub fn apply_history_key(
    history: &mut studio_core::History,
    op: HistoryOp,
    gesture_befores: &mut [&mut Option<Project>],
) -> bool {
    if op == HistoryOp::Undo {
        for before in gesture_befores.iter_mut() {
            flush_gesture_snapshot(history, before);
        }
    }
    apply_history_op(history, op)
}

/// Undo/Redo キーの押下エッジ追跡（Issue #31 レビュー対応）。
///
/// 発火は Z/Y キー自体の押下エッジでのみ行い、操作はその瞬間の
/// 修飾キーで決める。組み合わせの「現在の解釈」でレベル判定すると、
/// Ctrl+Shift+Z の後に Shift を先に離しただけで undo が誤発する
/// （redo を打ち消す）ため、この方式にする。編集中は発火せず、
/// 押下状態だけを同期する（ガード）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryKeyTracker {
    z_down: bool,
    y_down: bool,
}

impl HistoryKeyTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// 現在のキー状態を受け取り、発火すべき履歴操作があれば返す。
    pub fn update(
        &mut self,
        editing: bool,
        ctrl: bool,
        shift: bool,
        z_down: bool,
        y_down: bool,
    ) -> Option<HistoryOp> {
        let z_edge = z_down && !self.z_down;
        let y_edge = y_down && !self.y_down;
        self.z_down = z_down;
        self.y_down = y_down;
        if editing {
            return None;
        }
        if z_edge {
            resolve_history_op(false, ctrl, shift, true, false)
        } else if y_edge {
            resolve_history_op(false, ctrl, shift, false, true)
        } else {
            None
        }
    }

    /// 発火させずに押下状態だけを同期する（編集中用）。
    pub fn sync(&mut self, z_down: bool, y_down: bool) {
        self.z_down = z_down;
        self.y_down = y_down;
    }
}

/// ヒットテストの結果。
#[derive(Debug, Clone, PartialEq)]
pub enum HitTarget {
    ClipBody {
        clip_id: u64,
    },
    ClipLeftEdge {
        clip_id: u64,
    },
    ClipRightEdge {
        clip_id: u64,
    },
    /// トラック行の何もない場所（配置先）。
    Empty {
        track_index: usize,
    },
    /// タイムライン領域の外（ルーラー、ヘッダー、トラック行より下）。
    Outside,
}

/// x ピクセルを時刻（ms）に変換する。ヘッダーより左では負になる。
pub fn x_to_ms(x: f32, view_width_px: f32) -> i64 {
    ((x - timeline::HEADER_WIDTH_PX) / timeline::px_per_ms(view_width_px)).round() as i64
}

/// 座標（左上原点ピクセル）に対するヒットテスト。
pub fn hit_test(project: &Project, view_width_px: f32, x: f32, y: f32) -> HitTarget {
    if y < timeline::RULER_HEIGHT_PX || x < timeline::HEADER_WIDTH_PX {
        return HitTarget::Outside;
    }
    let track_index = ((y - timeline::RULER_HEIGHT_PX) / timeline::TRACK_HEIGHT_PX) as usize;
    if track_index >= project.tracks.len() {
        return HitTarget::Outside;
    }
    for rect in timeline::layout_clips(project, view_width_px) {
        if rect.track_id != project.tracks[track_index].id {
            continue;
        }
        if (x - rect.x).abs() <= EDGE_GRAB_PX {
            return HitTarget::ClipLeftEdge {
                clip_id: rect.clip_id,
            };
        }
        if (x - (rect.x + rect.w)).abs() <= EDGE_GRAB_PX {
            return HitTarget::ClipRightEdge {
                clip_id: rect.clip_id,
            };
        }
        if x > rect.x && x < rect.x + rect.w {
            return HitTarget::ClipBody {
                clip_id: rect.clip_id,
            };
        }
    }
    HitTarget::Empty { track_index }
}

/// ドラッグの種類。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragKind {
    Move,
    TrimLeft,
    TrimRight,
}

/// ドラッグ中の状態。開始時点のスナップショットを持つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Drag {
    pub clip_id: u64,
    pub kind: DragKind,
    pub start_pointer_ms: i64,
    pub orig_start_ms: i64,
    pub orig_duration_ms: i64,
}

impl Drag {
    /// ヒット結果とプロジェクトからドラッグを開始する。
    /// クリップ対象でなければ None。
    pub fn begin(project: &Project, target: &HitTarget, pointer_ms: i64) -> Option<Drag> {
        let (clip_id, kind) = match target {
            HitTarget::ClipBody { clip_id } => (*clip_id, DragKind::Move),
            HitTarget::ClipLeftEdge { clip_id } => (*clip_id, DragKind::TrimLeft),
            HitTarget::ClipRightEdge { clip_id } => (*clip_id, DragKind::TrimRight),
            _ => return None,
        };
        let (_, clip) = project.clip(clip_id)?;
        Some(Drag {
            clip_id,
            kind,
            start_pointer_ms: pointer_ms,
            orig_start_ms: clip.start_ms,
            orig_duration_ms: clip.duration_ms,
        })
    }

    /// 現在のポインタ位置に対する候補コマンド。
    pub fn command_for(&self, pointer_ms: i64) -> Command {
        let delta = pointer_ms - self.start_pointer_ms;
        match self.kind {
            DragKind::Move => Command::MoveClip {
                clip_id: self.clip_id,
                new_start_ms: self.orig_start_ms + delta,
            },
            DragKind::TrimLeft => Command::TrimClip {
                clip_id: self.clip_id,
                new_start_ms: self.orig_start_ms + delta,
                new_duration_ms: self.orig_duration_ms - delta,
            },
            DragKind::TrimRight => Command::TrimClip {
                clip_id: self.clip_id,
                new_start_ms: self.orig_start_ms,
                new_duration_ms: self.orig_duration_ms + delta,
            },
        }
    }
}

/// ダブルクリック判定。前回のクリック時刻と位置を保持する。
#[derive(Debug, Default)]
pub struct ClickTracker {
    last: Option<(u64, f32, f32)>,
}

impl ClickTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// クリックを登録し、ダブルクリックなら true を返す。
    pub fn register(&mut self, now_ms: u64, x: f32, y: f32) -> bool {
        let is_double = match self.last {
            Some((t, lx, ly)) => {
                now_ms.saturating_sub(t) <= DOUBLE_CLICK_MS
                    && (x - lx).abs() <= DOUBLE_CLICK_PX
                    && (y - ly).abs() <= DOUBLE_CLICK_PX
            }
            None => false,
        };
        // ダブルクリック成立後はリセットし、3連クリックを再度の
        // ダブルクリックと誤判定しない。
        self.last = if is_double {
            None
        } else {
            Some((now_ms, x, y))
        };
        is_double
    }
}

/// ヘッダー右端の削除帯の幅（px、Issue #57。DAW #56 と同じ）。
pub const DELETE_STRIP_PX: f32 = 18.0;

/// トラックヘッダー（名前のセル）のヒットテスト（Issue #57）。
/// 選択は GUI の表示状態であり、プロジェクト状態ではないため
/// コマンドには変換しない。
pub fn hit_track_header(project: &Project, x: f32, y: f32) -> Option<usize> {
    if x >= timeline::HEADER_WIDTH_PX || y < timeline::RULER_HEIGHT_PX {
        return None;
    }
    let track_index = ((y - timeline::RULER_HEIGHT_PX) / timeline::TRACK_HEIGHT_PX) as usize;
    if track_index < project.tracks.len() {
        Some(track_index)
    } else {
        None
    }
}

/// 「追加」領域のヒットテスト（Issue #57）。最終トラック行の直下の
/// ヘッダー列を上下に分け、上半分が映像、下半分が音声の追加。
/// 追加はコマンド経由で行い、ここは判定だけをする。
pub fn hit_track_add_area(project: &Project, x: f32, y: f32) -> Option<TrackKind> {
    if x >= timeline::HEADER_WIDTH_PX {
        return None;
    }
    let top = timeline::track_top_y(project.tracks.len());
    if y < top || y >= top + timeline::TRACK_HEIGHT_PX {
        return None;
    }
    if y < top + timeline::TRACK_HEIGHT_PX / 2.0 {
        Some(TrackKind::Video)
    } else {
        Some(TrackKind::Audio)
    }
}

/// ヘッダー右端の削除帯のヒットテスト（Issue #57）。
/// 当たったトラックの行インデックスを返す。
pub fn hit_track_delete(project: &Project, x: f32, y: f32) -> Option<usize> {
    if x < timeline::HEADER_WIDTH_PX - DELETE_STRIP_PX || x >= timeline::HEADER_WIDTH_PX {
        return None;
    }
    if y < timeline::RULER_HEIGHT_PX {
        return None;
    }
    let track_index = ((y - timeline::RULER_HEIGHT_PX) / timeline::TRACK_HEIGHT_PX) as usize;
    if track_index < project.tracks.len() {
        Some(track_index)
    } else {
        None
    }
}

/// トラック追加のコマンド（Issue #57）。名前は同じ種別の既存名の
/// 最大番号 +1 で、映像は「V1」「V2」…、音声は「A1」「A2」… と
/// 付ける（バグ #60 で採番規則を変更）。
pub fn add_track_command(project: &Project, kind: TrackKind) -> Command {
    let prefix = match kind {
        TrackKind::Video => "V",
        TrackKind::Audio => "A",
    };
    // 名前は同種別の既存名の最大番号 + 1（バグ #60: 既存数 + 1 では
    // 削除→追加で既存名と重複した）。「<接頭辞><数字>」の形でない
    // 名前は採番の対象にしない。
    let next = project
        .tracks
        .iter()
        .filter(|t| t.kind == kind)
        .filter_map(|t| t.name.strip_prefix(prefix)?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    Command::AddTrack {
        name: format!("{prefix}{next}"),
        kind,
    }
}

/// リネーム編集への入力（Issue #57。DAW #56 と同じ意味論）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenameInput {
    Char(char),
    Backspace,
    Enter,
    Escape,
}

/// リネーム編集セッションの結果（Issue #57）。
#[derive(Debug, Clone, PartialEq)]
pub enum RenameAction {
    /// 継続（確定も中止もしていない）。
    Continue,
    /// Enter で確定。発行すべきコマンド。
    Commit(Command),
    /// Esc で中止。コマンドは発行しない。
    Cancel,
}

/// リネームのインライン編集セッション（Issue #57）。GUI 側の表示状態
/// であり、プロジェクト状態には含めない。確定時にだけ RenameTrack
/// コマンドを返す。
#[derive(Debug, Clone, PartialEq)]
pub struct RenameSession {
    pub track_id: u64,
    pub text: String,
}

impl RenameSession {
    pub fn begin(track_id: u64, current_name: &str) -> RenameSession {
        RenameSession {
            track_id,
            text: current_name.to_string(),
        }
    }

    pub fn handle(&mut self, input: RenameInput) -> RenameAction {
        match input {
            RenameInput::Char(c) => {
                // 制御文字（Enter の '\r'、Esc、Backspace の '\u{8}' 等）は
                // 文字として取り込まない。確定・中止は KeyPressed 側で扱う。
                if !c.is_control() {
                    self.text.push(c);
                }
                RenameAction::Continue
            }
            RenameInput::Backspace => {
                self.text.pop();
                RenameAction::Continue
            }
            RenameInput::Enter => RenameAction::Commit(Command::RenameTrack {
                track_id: self.track_id,
                name: self.text.clone(),
            }),
            RenameInput::Escape => RenameAction::Cancel,
        }
    }
}

/// 空き場所への配置コマンドを作る。トラックが無ければ None。
pub fn place_clip_command(project: &Project, track_index: usize, at_ms: i64) -> Option<Command> {
    let track = project.tracks.get(track_index)?;
    let source = match track.kind {
        studio_core::TrackKind::Video => "demo/dropped.mp4",
        studio_core::TrackKind::Audio => "demo/dropped.wav",
    };
    Some(Command::AddClip {
        track_id: track.id,
        name: format!("clip-{}", track.clips.len() + 1),
        start_ms: at_ms,
        duration_ms: NEW_CLIP_DURATION_MS,
        source: source.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::{apply, TrackKind};

    fn sample_project() -> Project {
        let mut project = Project::new("test");
        let v = apply(
            &mut project,
            &Command::AddTrack {
                name: "V1".into(),
                kind: TrackKind::Video,
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut project,
            &Command::AddClip {
                track_id: v,
                name: "opening".into(),
                start_ms: 1_000,
                duration_ms: 2_000,
                source: "demo/opening.mp4".into(),
            },
        )
        .unwrap();
        project
    }

    fn clip_id(project: &Project) -> u64 {
        project.tracks[0].clips[0].id
    }

    // opening の矩形: x = 120 + 1000*0.084 = 204, w = 168, y = 24..72（幅 960）
    #[test]
    fn hit_test_distinguishes_body_edges_and_empty() {
        let p = sample_project();
        let id = clip_id(&p);
        assert_eq!(
            hit_test(&p, 960.0, 280.0, 48.0),
            HitTarget::ClipBody { clip_id: id }
        );
        assert_eq!(
            hit_test(&p, 960.0, 206.0, 48.0),
            HitTarget::ClipLeftEdge { clip_id: id }
        );
        assert_eq!(
            hit_test(&p, 960.0, 370.0, 48.0),
            HitTarget::ClipRightEdge { clip_id: id }
        );
        assert_eq!(
            hit_test(&p, 960.0, 700.0, 48.0),
            HitTarget::Empty { track_index: 0 }
        );
        assert_eq!(hit_test(&p, 960.0, 280.0, 10.0), HitTarget::Outside);
        assert_eq!(hit_test(&p, 960.0, 60.0, 48.0), HitTarget::Outside);
        assert_eq!(hit_test(&p, 960.0, 280.0, 200.0), HitTarget::Outside);
    }

    #[test]
    fn drag_move_uses_total_delta() {
        let p = sample_project();
        let id = clip_id(&p);
        let drag = Drag::begin(&p, &HitTarget::ClipBody { clip_id: id }, 1_500).unwrap();
        assert_eq!(
            drag.command_for(2_000),
            Command::MoveClip {
                clip_id: id,
                new_start_ms: 1_500
            }
        );
    }

    #[test]
    fn drag_trim_left_keeps_end_fixed() {
        let p = sample_project();
        let id = clip_id(&p);
        let drag = Drag::begin(&p, &HitTarget::ClipLeftEdge { clip_id: id }, 1_000).unwrap();
        // +300ms 動かす: start 1000→1300, duration 2000→1700, end は 3000 のまま
        assert_eq!(
            drag.command_for(1_300),
            Command::TrimClip {
                clip_id: id,
                new_start_ms: 1_300,
                new_duration_ms: 1_700
            }
        );
    }

    #[test]
    fn drag_trim_right_changes_duration_only() {
        let p = sample_project();
        let id = clip_id(&p);
        let drag = Drag::begin(&p, &HitTarget::ClipRightEdge { clip_id: id }, 3_000).unwrap();
        assert_eq!(
            drag.command_for(2_500),
            Command::TrimClip {
                clip_id: id,
                new_start_ms: 1_000,
                new_duration_ms: 1_500
            }
        );
    }

    #[test]
    fn drag_beyond_zero_produces_negative_candidate() {
        let p = sample_project();
        let id = clip_id(&p);
        let drag = Drag::begin(&p, &HitTarget::ClipBody { clip_id: id }, 1_500).unwrap();
        // 開始位置 1000ms のクリップを -2000ms 分動かすと候補は負になる。
        // 弾くのはコマンド層の責任（このモジュールは候補を返すだけ）。
        assert_eq!(
            drag.command_for(-500),
            Command::MoveClip {
                clip_id: id,
                new_start_ms: -1_000
            }
        );
    }

    #[test]
    fn double_click_detection() {
        let mut tracker = ClickTracker::new();
        assert!(!tracker.register(1_000, 300.0, 48.0));
        assert!(tracker.register(1_300, 302.0, 49.0));
        // 間隔が空けばダブルクリックではない
        let mut tracker = ClickTracker::new();
        assert!(!tracker.register(1_000, 300.0, 48.0));
        assert!(!tracker.register(1_600, 300.0, 48.0));
    }

    #[test]
    fn place_clip_command_builds_add_clip() {
        let p = sample_project();
        let track_id = p.tracks[0].id;
        let cmd = place_clip_command(&p, 0, 3_000).unwrap();
        match cmd {
            Command::AddClip {
                track_id: t,
                start_ms,
                duration_ms,
                ..
            } => {
                assert_eq!(t, track_id);
                assert_eq!(start_ms, 3_000);
                assert_eq!(duration_ms, NEW_CLIP_DURATION_MS);
            }
            other => panic!("expected AddClip, got {other:?}"),
        }
        assert!(place_clip_command(&p, 9, 0).is_none());
    }

    // Issue #57: トラック追加・削除・リネームの当たり判定と変換。

    fn two_track_project() -> Project {
        let mut p = Project::new("test57");
        for (name, kind) in [("V1", TrackKind::Video), ("A1", TrackKind::Audio)] {
            apply(
                &mut p,
                &Command::AddTrack {
                    name: name.into(),
                    kind,
                },
            )
            .unwrap();
        }
        p
    }

    #[test]
    fn hit_track_add_area_returns_kind() {
        let p = two_track_project(); // V1, A1 の2トラック
        let add_top = timeline::track_top_y(2);
        // 追加行ヘッダーの上半分が映像、下半分が音声
        assert_eq!(
            crate::edit::hit_track_add_area(&p, 60.0, add_top + 12.0),
            Some(TrackKind::Video)
        );
        assert_eq!(
            crate::edit::hit_track_add_area(&p, 60.0, add_top + 36.0),
            Some(TrackKind::Audio)
        );
        // タイムライン側は対象外
        assert_eq!(
            crate::edit::hit_track_add_area(&p, 300.0, add_top + 12.0),
            None
        );
        // トラック行やルーラーは対象外
        assert_eq!(crate::edit::hit_track_add_area(&p, 60.0, 48.0), None);
        assert_eq!(crate::edit::hit_track_add_area(&p, 60.0, 10.0), None);
        // 追加行より下は対象外
        assert_eq!(
            crate::edit::hit_track_add_area(&p, 60.0, timeline::track_top_y(3) + 10.0),
            None
        );
    }

    #[test]
    fn hit_track_delete_area() {
        let p = two_track_project();
        let x = timeline::HEADER_WIDTH_PX - 9.0;
        assert_eq!(crate::edit::hit_track_delete(&p, x, 48.0), Some(0));
        assert_eq!(crate::edit::hit_track_delete(&p, x, 96.0), Some(1));
        // 帯より左は対象外
        assert_eq!(
            crate::edit::hit_track_delete(
                &p,
                timeline::HEADER_WIDTH_PX - DELETE_STRIP_PX - 1.0,
                48.0
            ),
            None
        );
        // ルーラーと追加行は対象外
        assert_eq!(crate::edit::hit_track_delete(&p, x, 10.0), None);
        assert_eq!(
            crate::edit::hit_track_delete(&p, x, timeline::track_top_y(2) + 10.0),
            None
        );
    }

    #[test]
    fn add_track_command_names_by_kind() {
        let p = two_track_project(); // V1, A1
        match crate::edit::add_track_command(&p, TrackKind::Video) {
            Command::AddTrack { name, kind } => {
                assert_eq!(name, "V2");
                assert_eq!(kind, TrackKind::Video);
            }
            other => panic!("expected AddTrack, got {other:?}"),
        }
        match crate::edit::add_track_command(&p, TrackKind::Audio) {
            Command::AddTrack { name, kind } => {
                assert_eq!(name, "A2");
                assert_eq!(kind, TrackKind::Audio);
            }
            other => panic!("expected AddTrack, got {other:?}"),
        }
    }

    #[test]
    fn add_track_command_skips_numbers_of_removed_tracks() {
        // バグ #60: V1・V2 がある状態で V1 を削除して追加すると、
        // 既存数 + 1 では「V2」が重複した。最大番号 + 1 で V3 になる。
        let mut p = Project::new("t");
        let v1 = apply(
            &mut p,
            &Command::AddTrack {
                name: "V1".into(),
                kind: TrackKind::Video,
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::AddTrack {
                name: "V2".into(),
                kind: TrackKind::Video,
            },
        )
        .unwrap();
        apply(&mut p, &Command::RemoveTrack { track_id: v1 }).unwrap();
        match crate::edit::add_track_command(&p, TrackKind::Video) {
            Command::AddTrack { name, .. } => assert_eq!(name, "V3"),
            other => panic!("expected AddTrack, got {other:?}"),
        }
    }

    #[test]
    fn rename_session_enter_commits_and_esc_cancels() {
        let p = sample_project();
        let track_id = p.tracks[0].id;
        let mut session = RenameSession::begin(track_id, "V1");
        assert_eq!(
            session.handle(RenameInput::Char('2')),
            RenameAction::Continue
        );
        assert_eq!(session.text, "V12");
        assert_eq!(
            session.handle(RenameInput::Backspace),
            RenameAction::Continue
        );
        assert_eq!(session.text, "V1");
        match session.handle(RenameInput::Enter) {
            RenameAction::Commit(cmd) => assert_eq!(
                cmd,
                Command::RenameTrack {
                    track_id,
                    name: "V1".into()
                }
            ),
            other => panic!("expected Commit, got {other:?}"),
        }
        let mut session = RenameSession::begin(track_id, "V1");
        session.handle(RenameInput::Char('X'));
        assert_eq!(session.handle(RenameInput::Escape), RenameAction::Cancel);
    }
    #[test]
    fn ctrl_z_restores_state_before_last_edit() {
        // Issue #31（指定テスト）: Ctrl+Z で直前の編集前に戻る。
        let mut history = studio_core::History::new(sample_project());
        let clip_id = history.project().tracks[0].clips[0].id;
        let before = history.project().dump_json();
        history
            .apply(&Command::MoveClip {
                clip_id,
                new_start_ms: 2_500,
            })
            .unwrap();
        assert_ne!(history.project().dump_json(), before);
        let op = resolve_history_op(false, true, false, true, false)
            .expect("Ctrl+Z は undo として解決される");
        assert_eq!(op, HistoryOp::Undo);
        assert!(apply_history_op(&mut history, op));
        assert_eq!(history.project().dump_json(), before);
    }

    #[test]
    fn history_keys_cover_redo_and_editing_guard() {
        // Issue #31: Ctrl+Shift+Z と Ctrl+Y は redo。編集中と Ctrl なしは対象外。
        assert_eq!(
            resolve_history_op(false, true, true, true, false),
            Some(HistoryOp::Redo)
        );
        assert_eq!(
            resolve_history_op(false, true, false, false, true),
            Some(HistoryOp::Redo)
        );
        assert_eq!(resolve_history_op(true, true, false, true, false), None);
        assert_eq!(resolve_history_op(false, false, false, true, false), None);

        let mut history = studio_core::History::new(sample_project());
        let clip_id = history.project().tracks[0].clips[0].id;
        history
            .apply(&Command::MoveClip {
                clip_id,
                new_start_ms: 2_500,
            })
            .unwrap();
        let applied = history.project().dump_json();
        assert!(apply_history_op(&mut history, HistoryOp::Undo));
        assert!(apply_history_op(&mut history, HistoryOp::Redo));
        assert_eq!(history.project().dump_json(), applied);
    }
    #[test]
    fn history_key_tracker_fires_on_key_press_edge_only() {
        // Issue #31 レビュー対応: 発火は Z/Y の押下エッジのみ。
        // Ctrl+Shift+Z で redo した後、Z より先に Shift を離しても
        // undo は発火しない（旧方式はレベル判定で誤発した）。
        let mut tracker = HistoryKeyTracker::new();
        assert_eq!(
            tracker.update(false, true, true, true, false),
            Some(HistoryOp::Redo)
        );
        assert_eq!(tracker.update(false, true, false, true, false), None);
        assert_eq!(tracker.update(false, false, false, false, false), None);
        // Ctrl+Z で undo。押したまま Shift を足しても redo は誤発しない。
        assert_eq!(
            tracker.update(false, true, false, true, false),
            Some(HistoryOp::Undo)
        );
        assert_eq!(tracker.update(false, true, true, true, false), None);
        assert_eq!(tracker.update(false, false, false, false, false), None);
        // Ctrl+Y でも redo できる。
        assert_eq!(
            tracker.update(false, true, false, false, true),
            Some(HistoryOp::Redo)
        );
    }

    #[test]
    fn history_key_tracker_respects_editing_guard() {
        // Issue #31 レビュー対応: 編集中の押下エッジは発火せず、編集を
        // 終えても（キーを離すまで）持ち越し発火しない。
        let mut tracker = HistoryKeyTracker::new();
        assert_eq!(tracker.update(true, true, false, true, false), None);
        assert_eq!(tracker.update(false, true, false, true, false), None);
        assert_eq!(tracker.update(false, false, false, false, false), None);
        assert_eq!(
            tracker.update(false, true, false, true, false),
            Some(HistoryOp::Undo)
        );
    }
}
