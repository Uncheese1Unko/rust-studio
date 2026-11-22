//! ポインタ操作 → コマンド変換（Issue #14）。
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
/// フェードコーナーとみなす上端帯の高さ（px、Issue #34）。
pub const FADE_CORNER_PX: f32 = 12.0;
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

/// 複製・コピー＆ペーストのキー操作（Issue #33）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOp {
    Duplicate,
    Copy,
    Paste,
}

/// Ctrl+D / Ctrl+C / Ctrl+V の押下エッジ追跡（Issue #33）。
/// 履歴キーと同じく、発火はキー自体の押下エッジでのみ行い、
/// 編集中のエッジは消費して持ち越さない。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditKeyTracker {
    d_down: bool,
    c_down: bool,
    v_down: bool,
}

impl EditKeyTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// 現在のキー状態を受け取り、発火すべき編集操作があれば返す。
    pub fn update(
        &mut self,
        editing: bool,
        ctrl: bool,
        d_down: bool,
        c_down: bool,
        v_down: bool,
    ) -> Option<EditOp> {
        let d_edge = d_down && !self.d_down;
        let c_edge = c_down && !self.c_down;
        let v_edge = v_down && !self.v_down;
        self.d_down = d_down;
        self.c_down = c_down;
        self.v_down = v_down;
        if editing || !ctrl {
            return None;
        }
        if d_edge {
            Some(EditOp::Duplicate)
        } else if c_edge {
            Some(EditOp::Copy)
        } else if v_edge {
            Some(EditOp::Paste)
        } else {
            None
        }
    }

    /// 発火させずに押下状態だけを同期する（編集中用）。
    pub fn sync(&mut self, d_down: bool, c_down: bool, v_down: bool) {
        self.d_down = d_down;
        self.c_down = c_down;
        self.v_down = v_down;
    }
}

/// クリップの配置計画（Issue #33）。複製・貼り付けは新しいコア
/// コマンドを作らず、コピー元のパラメータを使った AddClip と、
/// 適用で得た新クリップ id へのパラメータ設定コマンドで表す。
/// ただし `offset_ms` は設定コマンドが存在しないため引き継げない
/// （分割したクリップの複製では素材の先頭から再生される。
/// フォローアップ PBI で扱う）。
#[derive(Debug, Clone, PartialEq)]
pub struct ClipPlacement {
    pub add: Command,
    gain: f32,
    pitch_semitones: f32,
    cutout: bool,
    color: Option<studio_core::ColorTag>,
    fade_in_ms: i64,
    fade_out_ms: i64,
}

impl ClipPlacement {
    fn from_clip(clip: &studio_core::Clip, track_id: u64, start_ms: i64) -> Self {
        ClipPlacement {
            add: Command::AddClip {
                track_id,
                name: clip.name.clone(),
                start_ms,
                duration_ms: clip.duration_ms,
                source: clip.source.clone(),
            },
            gain: clip.gain,
            pitch_semitones: clip.pitch_semitones,
            cutout: clip.cutout,
            color: clip.color,
            fade_in_ms: clip.fade_in_ms,
            fade_out_ms: clip.fade_out_ms,
        }
    }

    /// AddClip の適用で得た新クリップ id へ、コピー元のパラメータを
    /// 設定するコマンド列。既定値のパラメータはコマンドを出さない。
    pub fn follow_up_commands(&self, new_clip_id: u64) -> Vec<Command> {
        let mut cmds = Vec::new();
        if self.gain != 1.0 {
            cmds.push(Command::SetClipGain {
                clip_id: new_clip_id,
                gain: self.gain,
            });
        }
        if self.pitch_semitones != 0.0 {
            cmds.push(Command::SetClipPitch {
                clip_id: new_clip_id,
                semitones: self.pitch_semitones,
            });
        }
        if self.cutout {
            cmds.push(Command::SetClipCutout {
                clip_id: new_clip_id,
                enabled: true,
            });
        }
        if self.color.is_some() {
            cmds.push(Command::SetClipColor {
                clip_id: new_clip_id,
                color: self.color,
            });
        }
        if self.fade_in_ms != 0 || self.fade_out_ms != 0 {
            cmds.push(Command::SetClipFade {
                clip_id: new_clip_id,
                fade_in_ms: self.fade_in_ms,
                fade_out_ms: self.fade_out_ms,
            });
        }
        cmds
    }
}

/// 複製の配置計画（Issue #33）: 元クリップの終端に接する位置へ、
/// 同じトラックに置く。未知のクリップは None。
pub fn duplicate_placement(project: &Project, clip_id: u64) -> Option<ClipPlacement> {
    let (track, clip) = project.clip(clip_id)?;
    Some(ClipPlacement::from_clip(clip, track.id, clip.end_ms()))
}

/// 貼り付けの配置計画（Issue #33）: クリップボードのクリップを
/// 指定トラックの指定位置へ置く。
pub fn paste_placement(clipboard: &studio_core::Clip, track_id: u64, at_ms: i64) -> ClipPlacement {
    ClipPlacement::from_clip(clipboard, track_id, at_ms)
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
    /// クリップ上端の左コーナー（フェードインの掴み位置、Issue #34）。
    ClipFadeInCorner {
        clip_id: u64,
    },
    /// クリップ上端の右コーナー（フェードアウトの掴み位置、Issue #34）。
    ClipFadeOutCorner {
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
pub fn x_to_ms(x: f32, view_width_px: f32, scale: &timeline::ViewScale) -> i64 {
    scale.x_to_ms(x, view_width_px)
}

/// 座標（左上原点ピクセル）に対するヒットテスト。
pub fn hit_test(
    project: &Project,
    view_width_px: f32,
    scale: &timeline::ViewScale,
    x: f32,
    y: f32,
) -> HitTarget {
    if y < timeline::RULER_HEIGHT_PX || x < timeline::HEADER_WIDTH_PX {
        return HitTarget::Outside;
    }
    let track_index = ((y - timeline::RULER_HEIGHT_PX) / timeline::TRACK_HEIGHT_PX) as usize;
    if track_index >= project.tracks.len() {
        return HitTarget::Outside;
    }
    for rect in timeline::layout_clips(project, view_width_px, scale) {
        if rect.track_id != project.tracks[track_index].id {
            continue;
        }
        // 上端帯の左右コーナーはフェードの掴み位置（Issue #34）。
        // 同じ x でも上端帯より下は従来どおりトリム端になる。
        let top_band = y >= rect.y && y < rect.y + FADE_CORNER_PX;
        if top_band && (x - rect.x).abs() <= EDGE_GRAB_PX {
            return HitTarget::ClipFadeInCorner {
                clip_id: rect.clip_id,
            };
        }
        if top_band && (x - (rect.x + rect.w)).abs() <= EDGE_GRAB_PX {
            return HitTarget::ClipFadeOutCorner {
                clip_id: rect.clip_id,
            };
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

/// トラックヘッダー（名前のセル）のヒットテスト（Issue #25）。
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

/// ドラッグの種類。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragKind {
    Move,
    TrimLeft,
    TrimRight,
    /// フェードインの長さを変える（Issue #34）。
    FadeIn,
    /// フェードアウトの長さを変える（Issue #34）。
    FadeOut,
}

/// ドラッグ中の状態。開始時点のスナップショットを持つ。
#[derive(Debug, Clone, PartialEq)]
pub struct Drag {
    pub clip_id: u64,
    pub kind: DragKind,
    pub start_pointer_ms: i64,
    pub orig_start_ms: i64,
    pub orig_duration_ms: i64,
    /// ドラッグ開始時点のフェード長（Issue #34）。
    pub orig_fade_in_ms: i64,
    pub orig_fade_out_ms: i64,
}

impl Drag {
    /// ヒット結果とプロジェクトからドラッグを開始する。
    /// クリップ対象でなければ None。
    pub fn begin(project: &Project, target: &HitTarget, pointer_ms: i64) -> Option<Drag> {
        let (clip_id, kind) = match target {
            HitTarget::ClipBody { clip_id } => (*clip_id, DragKind::Move),
            HitTarget::ClipLeftEdge { clip_id } => (*clip_id, DragKind::TrimLeft),
            HitTarget::ClipRightEdge { clip_id } => (*clip_id, DragKind::TrimRight),
            HitTarget::ClipFadeInCorner { clip_id } => (*clip_id, DragKind::FadeIn),
            HitTarget::ClipFadeOutCorner { clip_id } => (*clip_id, DragKind::FadeOut),
            _ => return None,
        };
        let (_, clip) = project.clip(clip_id)?;
        Some(Drag {
            clip_id,
            kind,
            start_pointer_ms: pointer_ms,
            orig_start_ms: clip.start_ms,
            orig_duration_ms: clip.duration_ms,
            orig_fade_in_ms: clip.fade_in_ms,
            orig_fade_out_ms: clip.fade_out_ms,
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
            // フェードは 0..=duration にクランプした候補を出す
            // （検証はコマンド層でも行う、Issue #34）。
            DragKind::FadeIn => Command::SetClipFade {
                clip_id: self.clip_id,
                fade_in_ms: (self.orig_fade_in_ms + delta).clamp(0, self.orig_duration_ms),
                fade_out_ms: self.orig_fade_out_ms,
            },
            DragKind::FadeOut => Command::SetClipFade {
                clip_id: self.clip_id,
                fade_in_ms: self.orig_fade_in_ms,
                fade_out_ms: (self.orig_fade_out_ms - delta).clamp(0, self.orig_duration_ms),
            },
        }
    }

    /// スナップを適用した候補コマンド（Issue #37）。移動は開始位置、
    /// トリムは動かしている端が吸着する。フェードは対象外。
    /// `snap_enabled` が false（Alt 押下中）のときは候補がそのまま返る。
    pub fn snapped_command_for(
        &self,
        pointer_ms: i64,
        ctx: &SnapContext,
        snap_enabled: bool,
    ) -> Command {
        let raw = self.command_for(pointer_ms);
        if !snap_enabled {
            return raw;
        }
        match self.kind {
            DragKind::Move => match raw {
                Command::MoveClip {
                    clip_id,
                    new_start_ms,
                } => Command::MoveClip {
                    clip_id,
                    new_start_ms: snap_target(new_start_ms, ctx),
                },
                other => other,
            },
            DragKind::TrimLeft => match raw {
                Command::TrimClip {
                    clip_id,
                    new_start_ms,
                    ..
                } => {
                    let snapped = snap_target(new_start_ms, ctx);
                    Command::TrimClip {
                        clip_id,
                        new_start_ms: snapped,
                        new_duration_ms: self.orig_start_ms + self.orig_duration_ms - snapped,
                    }
                }
                other => other,
            },
            DragKind::TrimRight => match raw {
                Command::TrimClip {
                    clip_id,
                    new_start_ms,
                    new_duration_ms,
                } => {
                    let snapped_end = snap_target(new_start_ms + new_duration_ms, ctx);
                    Command::TrimClip {
                        clip_id,
                        new_start_ms,
                        new_duration_ms: snapped_end - new_start_ms,
                    }
                }
                other => other,
            },
            _ => raw,
        }
    }
}

/// スナップのしきい値（画面上のピクセル、Issue #37）。
pub const SNAP_THRESHOLD_PX: f32 = 8.0;

/// スナップの文脈（Issue #37）: 吸着先の候補と、しきい値（ms）。
/// 候補は再生ヘッド、他クリップの端（開始・終了）、小節線（BPM から
/// 計算）。純粋関数 `snap_target` の入力として edit.rs 内で完結させる。
#[derive(Debug, Clone, PartialEq)]
pub struct SnapContext {
    pub playhead_ms: i64,
    pub edges: Vec<i64>,
    pub bpm: f32,
    pub threshold_ms: i64,
}

impl SnapContext {
    /// プロジェクトから文脈を作る。`exclude_clip_id`（ドラッグ中の
    /// クリップ）の端は候補に入れない。
    pub fn from_project(
        project: &Project,
        exclude_clip_id: u64,
        playhead_ms: i64,
        threshold_ms: i64,
    ) -> SnapContext {
        let mut edges = Vec::new();
        for track in &project.tracks {
            for clip in &track.clips {
                if clip.id != exclude_clip_id {
                    edges.push(clip.start_ms);
                    edges.push(clip.end_ms());
                }
            }
        }
        SnapContext {
            playhead_ms,
            edges,
            bpm: project.bpm,
            threshold_ms,
        }
    }
}

/// 位置を最も近い吸着先へ寄せる（Issue #37）。しきい値内に対象が
/// 無ければ元の位置を返す。優先順は 再生ヘッド → クリップ端 →
/// 小節線で、同距離ではこの順に選ぶ。
pub fn snap_target(target_ms: i64, ctx: &SnapContext) -> i64 {
    // (位置, 優先順位) 優先順位は小さいほど優先。
    let mut candidates: Vec<(i64, u8)> = vec![(ctx.playhead_ms, 0)];
    candidates.extend(ctx.edges.iter().map(|&e| (e, 1)));
    let bar_ms = crate::timeline::bar_duration_ms(ctx.bpm);
    if bar_ms > 0.0 {
        let k = (target_ms as f64 / bar_ms).floor() as i64;
        for idx in [k, k + 1] {
            candidates.push((crate::timeline::bar_start_ms(ctx.bpm, idx), 2));
        }
    }
    let best = candidates
        .iter()
        .map(|&(pos, pri)| ((target_ms - pos).abs(), pri, pos))
        .filter(|&(dist, _, _)| dist <= ctx.threshold_ms)
        .min_by_key(|&(dist, pri, _)| (dist, pri));
    match best {
        Some((_, _, pos)) => pos,
        None => target_ms,
    }
}

/// ダブルクリック判定。前回のクリック時刻と位置を保持する。
///
/// `now_ms` には実経過ミリ秒を渡すこと。呼び出し側は nannou の
/// `app.time`（起動時 Instant からの経過秒。dt の累積ではないため
/// フレーム落ちで圧縮されない）× 1000 を渡している（Issue #56
/// レビュー round 1 で原典を確認済み）。
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

/// ヘッダー右端の削除帯の幅（px、Issue #56）。
pub const DELETE_STRIP_PX: f32 = 18.0;

/// 「追加」領域のヒットテスト（Issue #56）。最終トラック行の直下の
/// ヘッダー列。追加はコマンド経由で行い、ここは判定だけをする。
pub fn hit_track_add_area(project: &Project, x: f32, y: f32) -> bool {
    if x >= timeline::HEADER_WIDTH_PX {
        return false;
    }
    let top = timeline::track_top_y(project.tracks.len());
    y >= top && y < top + timeline::TRACK_HEIGHT_PX
}

/// ヘッダー右端の削除帯のヒットテスト（Issue #56）。
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

/// トラック追加のコマンド（Issue #56）。追加できるのは Audio のみ
/// （イシューのスコープ）。名前は既存の音声トラック名の最大番号 +1 で
/// 「A1」「A2」… と付ける（バグ #60 で採番規則を変更）。
pub fn add_track_command(project: &Project) -> Command {
    // 名前は既存の音声トラック名の最大番号 + 1（バグ #60: 既存数 + 1
    // では削除→追加で既存名と重複した）。「A<数字>」の形でない名前は
    // 採番の対象にしない。
    let next = project
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio)
        .filter_map(|t| t.name.strip_prefix('A')?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1;
    Command::AddTrack {
        name: format!("A{next}"),
        kind: TrackKind::Audio,
    }
}

/// リネーム編集への入力（Issue #56）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenameInput {
    Char(char),
    Backspace,
    Enter,
    Escape,
}

/// リネーム編集セッションの結果（Issue #56）。
#[derive(Debug, Clone, PartialEq)]
pub enum RenameAction {
    /// 継続（確定も中止もしていない）。
    None,
    /// Enter で確定。発行すべきコマンド。
    Commit(Command),
    /// Esc で中止。コマンドは発行しない。
    Cancel,
}

/// リネームのインライン編集セッション（Issue #56）。GUI 側の表示状態
/// であり、プロジェクト状態には含めない（#25 の選択状態と同じ扱い）。
/// 確定時にだけ RenameTrack コマンドを返す。
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
                RenameAction::None
            }
            RenameInput::Backspace => {
                self.text.pop();
                RenameAction::None
            }
            RenameInput::Enter => RenameAction::Commit(Command::RenameTrack {
                track_id: self.track_id,
                name: self.text.clone(),
            }),
            RenameInput::Escape => RenameAction::Cancel,
        }
    }
}

/// BPM 入力セッションの結果（Issue #35）。
#[derive(Debug, Clone, PartialEq)]
pub enum BpmAction {
    /// 継続（確定も中止もしていない）。
    None,
    /// Enter で確定。発行すべきコマンド。
    Commit(Command),
    /// Esc で中止。コマンドは発行しない。
    Cancel,
}

/// BPM のインライン入力セッション（Issue #35）。リネームと同じく
/// GUI 側の表示状態で、プロジェクトには含めない。数字と小数点だけを
/// 受け、Enter で SetBpm を返す。範囲の検証はコマンド層が行う。
#[derive(Debug, Clone, PartialEq)]
pub struct BpmEdit {
    buffer: String,
}

impl BpmEdit {
    pub fn new(current_bpm: f32) -> BpmEdit {
        let buffer = if current_bpm.fract() == 0.0 {
            format!("{}", current_bpm as i64)
        } else {
            format!("{current_bpm}")
        };
        BpmEdit { buffer }
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn handle(&mut self, input: RenameInput) -> BpmAction {
        match input {
            RenameInput::Char(c) if c.is_ascii_digit() => {
                if self.buffer.len() < 6 {
                    self.buffer.push(c);
                }
                BpmAction::None
            }
            RenameInput::Char('.') => {
                if !self.buffer.contains('.') && self.buffer.len() < 6 {
                    self.buffer.push('.');
                }
                BpmAction::None
            }
            RenameInput::Char(_) => BpmAction::None,
            RenameInput::Backspace => {
                self.buffer.pop();
                BpmAction::None
            }
            RenameInput::Enter => match self.buffer.parse::<f32>() {
                Ok(bpm) => BpmAction::Commit(Command::SetBpm { bpm }),
                Err(_) => BpmAction::None,
            },
            RenameInput::Escape => BpmAction::Cancel,
        }
    }
}

/// 空き場所への配置コマンドを作る。トラックが無ければ None。
/// 配置先のソースは呼び出し側が渡す（DAW ではデモ素材の実パス）。
pub fn place_clip_command(
    project: &Project,
    track_index: usize,
    at_ms: i64,
    source: &str,
) -> Option<Command> {
    let track = project.tracks.get(track_index)?;
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
    fn x_to_ms_round_trips_at_any_zoom() {
        // Issue #38: 任意の倍率・オフセットで ms→px→ms が一致する
        // （ピクセル量子化の誤差以内）。
        for (zoom, offset) in [(1.0, 0), (2.0, 1_500), (0.5, 0), (8.0, 3_000), (0.25, 0)] {
            let scale = timeline::ViewScale {
                zoom,
                offset_ms: offset,
            };
            for ms in [0, 1_234, 5_000, 9_999] {
                let x = scale.time_to_x(ms, 960.0);
                let back = x_to_ms(x, 960.0, &scale);
                let tol = (0.5 / scale.px_per_ms(960.0)).ceil() as i64 + 1;
                assert!(
                    (back - ms).abs() <= tol,
                    "zoom {zoom} offset {offset}: {ms} -> {x} -> {back}"
                );
            }
        }
    }

    #[test]
    fn hit_test_uses_view_scale() {
        // Issue #38: ヒットテストは倍率・オフセットを反映する。
        // zoom=2・offset=1000 ではクリップ（1000..3000ms）は
        // x=120..456 に描かれる。x=130 は既定スケールでは空きだが
        // このスケールではクリップ本体。
        let p = sample_project();
        let id = clip_id(&p);
        let scale = timeline::ViewScale {
            zoom: 2.0,
            offset_ms: 1_000,
        };
        assert_eq!(
            hit_test(&p, 960.0, &scale, 130.0, 48.0),
            HitTarget::ClipBody { clip_id: id }
        );
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 130.0, 48.0),
            HitTarget::Empty { track_index: 0 }
        );
    }

    #[test]
    fn hit_test_distinguishes_body_edges_and_empty() {
        let p = sample_project();
        let id = clip_id(&p);
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 280.0, 48.0),
            HitTarget::ClipBody { clip_id: id }
        );
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 206.0, 48.0),
            HitTarget::ClipLeftEdge { clip_id: id }
        );
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 370.0, 48.0),
            HitTarget::ClipRightEdge { clip_id: id }
        );
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 700.0, 48.0),
            HitTarget::Empty { track_index: 0 }
        );
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 280.0, 10.0),
            HitTarget::Outside
        );
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 60.0, 48.0),
            HitTarget::Outside
        );
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 280.0, 200.0),
            HitTarget::Outside
        );
    }

    #[test]
    fn hit_test_top_corners_are_fade_handles() {
        // Issue #34: クリップ上端の左右コーナーはフェードの掴み位置で、
        // 同じ x でも上端帯より下は従来どおりトリム端になる。
        let p = sample_project();
        let id = clip_id(&p);
        // 左上コーナー（左端 x=204、上端帯 y=28）
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 206.0, 28.0),
            HitTarget::ClipFadeInCorner { clip_id: id }
        );
        // 右上コーナー（右端 x=372、上端帯 y=28）
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 370.0, 28.0),
            HitTarget::ClipFadeOutCorner { clip_id: id }
        );
        // 上端帯より下（y=48）はトリム端のまま
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 206.0, 48.0),
            HitTarget::ClipLeftEdge { clip_id: id }
        );
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 370.0, 48.0),
            HitTarget::ClipRightEdge { clip_id: id }
        );
    }

    #[test]
    fn drag_fade_corners_produce_set_clip_fade() {
        // Issue #34: フェードコーナーのドラッグは SetClipFade を作る。
        // フェードインは右へ引くほど長く、フェードアウトは左へ
        // 引くほど長くなる。範囲は 0..=duration にクランプされる。
        let p = sample_project();
        let id = clip_id(&p);
        let drag = Drag::begin(&p, &HitTarget::ClipFadeInCorner { clip_id: id }, 1_000)
            .expect("fade-in drag");
        assert_eq!(
            drag.command_for(1_500),
            Command::SetClipFade {
                clip_id: id,
                fade_in_ms: 500,
                fade_out_ms: 0
            }
        );
        // 左へ引き戻すと負にはならず 0 にクランプ
        assert_eq!(
            drag.command_for(500),
            Command::SetClipFade {
                clip_id: id,
                fade_in_ms: 0,
                fade_out_ms: 0
            }
        );
        let drag = Drag::begin(&p, &HitTarget::ClipFadeOutCorner { clip_id: id }, 3_000)
            .expect("fade-out drag");
        assert_eq!(
            drag.command_for(2_500),
            Command::SetClipFade {
                clip_id: id,
                fade_in_ms: 0,
                fade_out_ms: 500
            }
        );
    }

    #[test]
    fn hit_track_header_selects_track_rows() {
        // Issue #25: トラックヘッダー（名前セル）のクリックで選択する。
        let p = sample_project();
        assert_eq!(hit_track_header(&p, 60.0, 48.0), Some(0));
        // ヘッダーより右（タイムライン本体）やルーラーは対象外
        assert_eq!(hit_track_header(&p, 300.0, 48.0), None);
        assert_eq!(hit_track_header(&p, 60.0, 10.0), None);
        // トラック行より下は対象外
        assert_eq!(hit_track_header(&p, 60.0, 200.0), None);
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
        let cmd = place_clip_command(&p, 0, 3_000, "demo/dropped.wav").unwrap();
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
        assert!(place_clip_command(&p, 9, 0, "demo/dropped.wav").is_none());
    }

    #[test]
    fn hit_track_add_area() {
        // Issue #56: 最終トラック行の直下、ヘッダー列が「追加」領域。
        let p = sample_project();
        let add_y = timeline::track_top_y(1) + 10.0;
        assert!(crate::edit::hit_track_add_area(&p, 60.0, add_y));
        // 既存トラックのヘッダーは追加領域ではない
        assert!(!crate::edit::hit_track_add_area(&p, 60.0, 48.0));
        // ヘッダー列より右は対象外
        assert!(!crate::edit::hit_track_add_area(&p, 300.0, add_y));
        // ルーラーは対象外
        assert!(!crate::edit::hit_track_add_area(&p, 60.0, 10.0));
        // 追加行より下は対象外
        assert!(!crate::edit::hit_track_add_area(
            &p,
            60.0,
            timeline::track_top_y(2) + 10.0
        ));
    }

    #[test]
    fn hit_track_delete_area() {
        // Issue #56: ヘッダー右端の帯が削除領域（行インデックスを返す）。
        let p = sample_project();
        let x = timeline::HEADER_WIDTH_PX - DELETE_STRIP_PX / 2.0;
        assert_eq!(hit_track_delete(&p, x, 48.0), Some(0));
        // 名前中央は削除領域ではない
        assert_eq!(hit_track_delete(&p, 60.0, 48.0), None);
        // タイムライン本体やルーラーは対象外
        assert_eq!(hit_track_delete(&p, 300.0, 48.0), None);
        assert_eq!(hit_track_delete(&p, x, 10.0), None);
        // トラック行より下は対象外
        assert_eq!(hit_track_delete(&p, x, 200.0), None);
    }

    #[test]
    fn add_track_command_uses_next_audio_name() {
        // Issue #56: 追加は Audio のみ。名前は既存の最大番号 +1。
        let p = sample_project();
        assert_eq!(
            add_track_command(&p),
            Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio
            }
        );
        let mut p = sample_project();
        apply(
            &mut p,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap();
        assert_eq!(
            add_track_command(&p),
            Command::AddTrack {
                name: "A2".into(),
                kind: TrackKind::Audio
            }
        );
    }

    #[test]
    fn add_track_command_skips_numbers_of_removed_tracks() {
        // バグ #60: A1・A2 がある状態で A1 を削除して追加すると、
        // 既存数 + 1 では「A2」が重複した。最大番号 + 1 で A3 になる。
        let mut p = Project::new("t");
        let a1 = apply(
            &mut p,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::AddTrack {
                name: "A2".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap();
        apply(&mut p, &Command::RemoveTrack { track_id: a1 }).unwrap();
        assert_eq!(
            add_track_command(&p),
            Command::AddTrack {
                name: "A3".into(),
                kind: TrackKind::Audio
            }
        );
    }

    #[test]
    fn rename_session_enter_commits_and_esc_cancels() {
        // Issue #56: Enter で RenameTrack コマンドが確定し、Esc では
        // 何も確定しない（コマンドが出ない）。
        let p = sample_project();
        let track_id = p.tracks[0].id;
        let mut session = RenameSession::begin(track_id, "V1");
        assert_eq!(session.handle(RenameInput::Char('2')), RenameAction::None);
        assert_eq!(session.text, "V12");
        assert_eq!(
            session.handle(RenameInput::Enter),
            RenameAction::Commit(Command::RenameTrack {
                track_id,
                name: "V12".into()
            })
        );
        let mut session = RenameSession::begin(track_id, "V1");
        session.handle(RenameInput::Char('X'));
        assert_eq!(session.handle(RenameInput::Escape), RenameAction::Cancel);
    }

    #[test]
    fn rename_session_edits_text() {
        // Issue #56: Backspace は末尾1文字を消し、制御文字は取り込まない。
        let p = sample_project();
        let track_id = p.tracks[0].id;
        let mut session = RenameSession::begin(track_id, "V1");
        session.handle(RenameInput::Backspace);
        assert_eq!(session.text, "V");
        session.handle(RenameInput::Char('\n'));
        session.handle(RenameInput::Char('\u{1b}'));
        assert_eq!(session.text, "V");
        // 空でも Backspace は壊れない
        session.handle(RenameInput::Backspace);
        session.handle(RenameInput::Backspace);
        assert_eq!(session.text, "");
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

    /// パラメータ付きクリップ（ゲイン・ピッチ・切り抜き・色）を用意する。
    fn project_with_tuned_clip() -> (Project, u64, u64) {
        let mut project = Project::new("test");
        let track = apply(
            &mut project,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        let clip = apply(
            &mut project,
            &Command::AddClip {
                track_id: track,
                name: "phrase".into(),
                start_ms: 1_000,
                duration_ms: 2_000,
                source: "phrase.wav".into(),
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut project,
            &Command::SetClipGain {
                clip_id: clip,
                gain: 0.5,
            },
        )
        .unwrap();
        apply(
            &mut project,
            &Command::SetClipPitch {
                clip_id: clip,
                semitones: 3.0,
            },
        )
        .unwrap();
        apply(
            &mut project,
            &Command::SetClipCutout {
                clip_id: clip,
                enabled: true,
            },
        )
        .unwrap();
        apply(
            &mut project,
            &Command::SetClipColor {
                clip_id: clip,
                color: Some(studio_core::ColorTag::Mint),
            },
        )
        .unwrap();
        (project, track, clip)
    }

    #[test]
    fn duplicate_command_copies_all_clip_parameters() {
        // Issue #33: 複製は元クリップの直後に置かれ、全パラメータ
        // （ゲイン・ピッチ・切り抜き・色）を引き継ぐ。
        let (mut project, _track, clip_id) = project_with_tuned_clip();
        let placement = duplicate_placement(&project, clip_id).expect("clip exists");
        match &placement.add {
            Command::AddClip {
                name,
                start_ms,
                duration_ms,
                source,
                ..
            } => {
                assert_eq!(name, "phrase");
                assert_eq!(*start_ms, 3_000, "元の終端に接する位置");
                assert_eq!(*duration_ms, 2_000);
                assert_eq!(source, "phrase.wav");
            }
            other => panic!("expected AddClip, got {other:?}"),
        }
        // 実際に適用すると、パラメータが一致した複製ができる。
        let new_id = apply(&mut project, &placement.add).unwrap().unwrap();
        for cmd in placement.follow_up_commands(new_id) {
            apply(&mut project, &cmd).unwrap();
        }
        let (_, src) = project.clip(clip_id).unwrap();
        let (_, dup) = project.clip(new_id).unwrap();
        assert_eq!(dup.gain, src.gain);
        assert_eq!(dup.pitch_semitones, src.pitch_semitones);
        assert_eq!(dup.cutout, src.cutout);
        assert_eq!(dup.color, src.color);
        assert_eq!(dup.duration_ms, src.duration_ms);
    }

    #[test]
    fn duplicate_placement_unknown_clip_is_none() {
        let (project, _, _) = project_with_tuned_clip();
        assert!(duplicate_placement(&project, 999).is_none());
    }

    #[test]
    fn paste_placement_puts_clip_at_playhead_on_target_track() {
        // Issue #33: 貼り付けは指定トラックの再生ヘッド位置に、
        // コピー元のパラメータで置かれる。
        let (project, track, clip_id) = project_with_tuned_clip();
        let (_, clip) = project.clip(clip_id).unwrap();
        let placement = paste_placement(clip, track, 4_200);
        match &placement.add {
            Command::AddClip {
                track_id,
                start_ms,
                duration_ms,
                ..
            } => {
                assert_eq!(*track_id, track);
                assert_eq!(*start_ms, 4_200);
                assert_eq!(*duration_ms, 2_000);
            }
            other => panic!("expected AddClip, got {other:?}"),
        }
        let follow = placement.follow_up_commands(42);
        assert!(follow.contains(&Command::SetClipGain {
            clip_id: 42,
            gain: 0.5
        }));
        assert!(follow.contains(&Command::SetClipPitch {
            clip_id: 42,
            semitones: 3.0
        }));
    }

    #[test]
    fn snap_picks_nearest_bar_line_within_threshold() {
        // Issue #37: しきい値内では小節線に吸着し、しきい値外では
        // 元の位置のまま。bpm=120 → 小節は 2000ms ごと、しきい値 95ms。
        let ctx = SnapContext {
            playhead_ms: -100_000,
            edges: vec![],
            bpm: 120.0,
            threshold_ms: 95,
        };
        assert_eq!(snap_target(1_950, &ctx), 2_000);
        assert_eq!(snap_target(2_050, &ctx), 2_000);
        assert_eq!(snap_target(1_850, &ctx), 1_850);
        assert_eq!(snap_target(90, &ctx), 0);
        assert_eq!(snap_target(100, &ctx), 100);
    }

    #[test]
    fn snap_prefers_playhead_then_clip_edge_on_ties() {
        // Issue #37: 優先順は 再生ヘッド → クリップ端 → 小節線。
        // 同距離ではこの順で選ぶ。
        let ctx = SnapContext {
            playhead_ms: 1_000,
            edges: vec![1_020],
            bpm: 120.0,
            threshold_ms: 95,
        };
        // ヘッド（距離10）と端（距離10）の同点ではヘッド
        assert_eq!(snap_target(1_010, &ctx), 1_000);
        // 端の方が近ければ端
        assert_eq!(snap_target(1_015, &ctx), 1_020);
        // 端（1990）と小節線（2000）の同点では端
        let ctx = SnapContext {
            playhead_ms: -100_000,
            edges: vec![1_990],
            bpm: 120.0,
            threshold_ms: 95,
        };
        assert_eq!(snap_target(1_995, &ctx), 1_990);
    }

    #[test]
    fn drag_move_and_trim_snap_through_context() {
        // Issue #37: 移動は開始位置、トリムは動かしている端が吸着する。
        // Alt で無効化した場合は候補がそのまま返る。
        let p = sample_project();
        let id = clip_id(&p);
        let ctx = SnapContext {
            playhead_ms: 500,
            edges: vec![],
            bpm: 120.0,
            threshold_ms: 95,
        };
        // 移動: raw 1980 → 小節線 2000 に吸着
        let drag = Drag::begin(&p, &HitTarget::ClipBody { clip_id: id }, 1_000).unwrap();
        assert_eq!(
            drag.snapped_command_for(1_980, &ctx, true),
            Command::MoveClip {
                clip_id: id,
                new_start_ms: 2_000
            }
        );
        // 無効化時は raw のまま
        assert_eq!(
            drag.snapped_command_for(1_980, &ctx, false),
            Command::MoveClip {
                clip_id: id,
                new_start_ms: 1_980
            }
        );
        // 左トリム: raw 520 → 再生ヘッド 500 に吸着し、終端は不変
        let drag = Drag::begin(&p, &HitTarget::ClipLeftEdge { clip_id: id }, 1_000).unwrap();
        assert_eq!(
            drag.snapped_command_for(520, &ctx, true),
            Command::TrimClip {
                clip_id: id,
                new_start_ms: 500,
                new_duration_ms: 2_500
            }
        );
        // 右トリム: raw 終端 4950 → 他クリップの端 5000 に吸着
        let mut p2 = sample_project();
        let track = p2.tracks[0].id;
        apply(
            &mut p2,
            &Command::AddClip {
                track_id: track,
                name: "next".into(),
                start_ms: 5_000,
                duration_ms: 1_000,
                source: "demo/next.mp4".into(),
            },
        )
        .unwrap();
        let ctx2 = SnapContext::from_project(&p2, id, -100_000, 95);
        let drag = Drag::begin(&p2, &HitTarget::ClipRightEdge { clip_id: id }, 3_000).unwrap();
        assert_eq!(
            drag.snapped_command_for(4_950, &ctx2, true),
            Command::TrimClip {
                clip_id: id,
                new_start_ms: 1_000,
                new_duration_ms: 4_000
            }
        );
    }

    #[test]
    fn bpm_edit_types_digits_and_commits_on_enter() {
        // Issue #35: BPM 入力は数字と小数点だけを受け、Enter で
        // SetBpm を発行する。Esc は取消。
        let mut edit = BpmEdit::new(120.0);
        assert_eq!(edit.buffer(), "120");
        edit.handle(RenameInput::Backspace);
        edit.handle(RenameInput::Backspace);
        edit.handle(RenameInput::Backspace);
        assert_eq!(edit.buffer(), "");
        // 空で Enter しても確定しない
        assert_eq!(edit.handle(RenameInput::Enter), BpmAction::None);
        edit.handle(RenameInput::Char('9'));
        edit.handle(RenameInput::Char('0'));
        // 数字以外は無視される
        assert_eq!(edit.handle(RenameInput::Char('x')), BpmAction::None);
        assert_eq!(edit.buffer(), "90");
        assert_eq!(
            edit.handle(RenameInput::Enter),
            BpmAction::Commit(Command::SetBpm { bpm: 90.0 })
        );
        // 小数点は1つまで
        let mut edit = BpmEdit::new(90.0);
        edit.handle(RenameInput::Char('.'));
        edit.handle(RenameInput::Char('.'));
        edit.handle(RenameInput::Char('5'));
        assert_eq!(edit.buffer(), "90.5");
        assert_eq!(edit.handle(RenameInput::Escape), BpmAction::Cancel);
    }

    #[test]
    fn edit_key_tracker_fires_on_ctrl_edges_only() {
        // Issue #33: Ctrl+D/C/V は押下エッジでのみ発火し、押しっぱなしや
        // Ctrl なしでは発火しない。編集中は発火せず持ち越さない。
        let mut tracker = EditKeyTracker::new();
        // (editing, ctrl, d, c, v)
        assert_eq!(tracker.update(false, false, true, false, false), None);
        assert_eq!(tracker.update(false, true, false, false, false), None);
        assert_eq!(
            tracker.update(false, true, true, false, false),
            Some(EditOp::Duplicate)
        );
        // 押しっぱなしでは再発火しない
        assert_eq!(tracker.update(false, true, true, false, false), None);
        assert_eq!(tracker.update(false, false, false, false, false), None);
        assert_eq!(
            tracker.update(false, true, false, true, false),
            Some(EditOp::Copy)
        );
        assert_eq!(tracker.update(false, false, false, false, false), None);
        assert_eq!(
            tracker.update(false, true, false, false, true),
            Some(EditOp::Paste)
        );
        assert_eq!(tracker.update(false, false, false, false, false), None);
        // 編集中のエッジは消費され、編集終了後に持ち越さない
        assert_eq!(tracker.update(true, true, true, false, false), None);
        assert_eq!(tracker.update(false, true, true, false, false), None);
    }
}
