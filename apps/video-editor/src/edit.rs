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

/// フェードコーナーと判定する上端帯の高さ（Issue #51、DAW #34 と同じ）。
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

/// 複製・コピー＆ペーストのキー操作（Issue #44）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOp {
    Duplicate,
    Copy,
    Paste,
}

/// Ctrl+D / Ctrl+C / Ctrl+V の押下エッジ追跡（Issue #44）。
/// 履歴キーと同じく、発火はキー自体の押下エッジでのみ行い、
/// 編集中のエッジは消費して持ち越さない。DAW（#33）と同一方式。
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

/// クリップの配置計画（Issue #44）。複製・貼り付けは新しいコア
/// コマンドを作らず、コピー元のパラメータを使った AddClip と、
/// 適用で得た新クリップ id へのパラメータ設定コマンドで表す
/// （DAW #33 と同一方針）。引き継ぐパラメータは gain / pitch /
/// cutout / color / fade / title / offset（Issue #63。offset は
/// #62 で新設された SetClipOffset で引き継ぐ）。
#[derive(Debug, Clone, PartialEq)]
pub struct ClipPlacement {
    pub add: Command,
    gain: f32,
    pitch_semitones: f32,
    cutout: bool,
    color: Option<studio_core::ColorTag>,
    fade_in_ms: i64,
    fade_out_ms: i64,
    // Issue #50: タイトルも「全パラメータ」に含める。
    title_text: String,
    // Issue #63: 素材内オフセットも引き継ぐ（#62 の SetClipOffset）。
    offset_ms: i64,
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
            title_text: clip.title_text.clone(),
            offset_ms: clip.offset_ms,
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
        if !self.title_text.is_empty() {
            cmds.push(Command::SetClipTitle {
                clip_id: new_clip_id,
                title: self.title_text.clone(),
            });
        }
        if self.offset_ms != 0 {
            cmds.push(Command::SetClipOffset {
                clip_id: new_clip_id,
                offset_ms: self.offset_ms,
            });
        }
        cmds
    }
}

/// 複製の配置計画（Issue #44）: 元クリップの終端に接する位置へ、
/// 同じトラックに置く。未知のクリップは None。
pub fn duplicate_placement(project: &Project, clip_id: u64) -> Option<ClipPlacement> {
    let (track, clip) = project.clip(clip_id)?;
    Some(ClipPlacement::from_clip(clip, track.id, clip.end_ms()))
}

/// 貼り付けの配置計画（Issue #44）: クリップボードのクリップを
/// 指定トラックの指定位置へ置く。
pub fn paste_placement(clipboard: &studio_core::Clip, track_id: u64, at_ms: i64) -> ClipPlacement {
    ClipPlacement::from_clip(clipboard, track_id, at_ms)
}

/// 配置計画を履歴へ「1回の操作」として適用する（Issue #44 レビュー
/// 対応）。まず現在の状態の複製に対して全コマンドを試行し、全部
/// 成功する場合だけ実プロジェクトへ適用するため、途中失敗で
/// 半端なパラメータのクリップが残ることはない。実適用は
/// `apply_transient` で行い、開始時スナップショットを1件だけ
/// 履歴に積むので、複製・貼り付け全体が1回の undo で戻せる
/// （コマンドごとに積むと、途中の undo でパラメータの一部だけ
/// 設定された複製が残ってしまう）。
pub fn apply_placement(
    history: &mut studio_core::History,
    placement: &ClipPlacement,
) -> Result<u64, studio_core::CommandError> {
    // 試行: 現在の状態の複製に対して全コマンドを適用する。
    let mut trial = history.project().clone();
    let trial_id =
        studio_core::apply(&mut trial, &placement.add)?.expect("AddClip returns the new clip id");
    for cmd in placement.follow_up_commands(trial_id) {
        studio_core::apply(&mut trial, &cmd)?;
    }
    // 確定: 試行が成功したコマンド列は、コマンドが状態に対して
    // 決定的であるため実プロジェクトでも成功する。
    let mut before = Some(history.project().clone());
    let new_id = history
        .apply_transient(&placement.add)
        .expect("trial succeeded")
        .expect("AddClip returns the new clip id");
    debug_assert_eq!(new_id, trial_id);
    for cmd in placement.follow_up_commands(new_id) {
        history.apply_transient(&cmd).expect("trial succeeded");
    }
    flush_gesture_snapshot(history, &mut before);
    Ok(new_id)
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
    /// クリップ上端の左コーナー（フェードインの掴み位置、Issue #51）。
    ClipFadeInCorner {
        clip_id: u64,
    },
    /// クリップ上端の右コーナー（フェードアウトの掴み位置、Issue #51）。
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

/// ゲインスライダーの割合から SetClipGain コマンドを作る
/// （Issue #51）。割合 0..=1 を ゲイン 0.0..=2.0 に対応させる
/// （中央 0.5 が等倍 1.0。ミキサーの音量範囲と同じ上限）。
/// 範囲外の割合は端に丸める。
pub fn gain_slider_command(clip_id: u64, fraction: f32) -> Command {
    let gain = fraction.clamp(0.0, 1.0) * 2.0;
    Command::SetClipGain { clip_id, gain }
}

/// ドロップで取り込める素材の種別（Issue #53）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropKind {
    Video,
    Image,
    Audio,
}

/// ドロップされたパスの種別（Issue #53）。対応拡張子は media.rs の
/// 利用対象（動画 mp4/mov、画像 png/jpg、音声 wav）。大文字小文字は
/// 問わない。対応外は None。
pub fn drop_kind(path: &std::path::Path) -> Option<DropKind> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    match ext.as_str() {
        "mp4" | "mov" => Some(DropKind::Video),
        "png" | "jpg" => Some(DropKind::Image),
        "wav" => Some(DropKind::Audio),
        _ => None,
    }
}

/// ドロップしたファイルの配置コマンド（Issue #53）。
/// #30 の調査でドロップ座標は取得できないことが確定したため、
/// 配置先は「素材種別に合うトラック（選択中トラックを優先、
/// 合わなければその種別の先頭トラック）＋再生ヘッド位置」とする
/// （DAW #42 の代替規則を、2種のトラックを持つ動画編集に合わせて
/// 種別マッチまで広げた）。合うトラックが無い・長さが 0 なら
/// 配置できない（None）。
pub fn drop_placement_command(
    project: &Project,
    selected_track: Option<u64>,
    playhead_ms: i64,
    source: &str,
    duration_ms: i64,
    kind: DropKind,
) -> Option<Command> {
    if duration_ms <= 0 {
        return None;
    }
    let want = match kind {
        DropKind::Video | DropKind::Image => TrackKind::Video,
        DropKind::Audio => TrackKind::Audio,
    };
    let track_id = selected_track
        .filter(|id| project.tracks.iter().any(|t| t.id == *id && t.kind == want))
        .or_else(|| project.tracks.iter().find(|t| t.kind == want).map(|t| t.id))?;
    let name = std::path::Path::new(source)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "clip".to_string());
    Some(Command::AddClip {
        track_id,
        name,
        start_ms: playhead_ms.max(0),
        duration_ms,
        source: source.to_string(),
    })
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
        // 幅 0 の矩形（表示範囲より完全に前のクリップ）は何にも
        // 当たらない（Issue #46 レビュー round 2 指摘）。
        if rect.w <= 0.0 {
            continue;
        }
        // 上端帯の左右コーナーはフェードの掴み位置（Issue #51、
        // DAW #34 と同じ規則）。同じ x でも帯より下は従来どおり
        // トリム端になる。左端が切り取られている場合は左コーナー
        // も実在しないため付けない（Issue #46 の左端と同じ扱い）。
        let top_band = y >= rect.y && y < rect.y + FADE_CORNER_PX;
        if top_band && !rect.clipped_left && (x - rect.x).abs() <= EDGE_GRAB_PX {
            return HitTarget::ClipFadeInCorner {
                clip_id: rect.clip_id,
            };
        }
        if top_band && (x - (rect.x + rect.w)).abs() <= EDGE_GRAB_PX {
            return HitTarget::ClipFadeOutCorner {
                clip_id: rect.clip_id,
            };
        }
        // 左端が画面外に切り取られている場合、その境界は実在しない
        // ため掴み位置にしない（同指摘）。
        if !rect.clipped_left && (x - rect.x).abs() <= EDGE_GRAB_PX {
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
    FadeIn,
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
    /// フェードドラッグ用（Issue #51）。他のドラッグでは未使用。
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
            // フェードは対象側だけを 0..=duration にクランプして
            // 変える（Issue #51、DAW #34 と同じ計算。検証はコマンド層）。
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

    /// スナップを適用した候補コマンド（Issue #45）。
    /// `snap_enabled` が false（Alt 押下中）なら候補をそのまま返す。
    /// 移動は開始端・終端の両方を評価し、より近い吸着を採用する。
    /// トリムは動かしている端だけを吸着させる。吸着後も範囲外の
    /// 候補（負の位置など）はそのまま返し、弾くのはコマンド層。
    pub fn snapped_command_for(
        &self,
        project: &Project,
        pointer_ms: i64,
        playhead_ms: i64,
        view_width_px: f32,
        scale: &timeline::ViewScale,
        snap_enabled: bool,
    ) -> Command {
        let base = self.command_for(pointer_ms);
        // フェードはスナップ対象外（DAW #37 と同じ）。
        if matches!(self.kind, DragKind::FadeIn | DragKind::FadeOut) {
            return base;
        }
        if !snap_enabled {
            return base;
        }
        match (self.kind, base) {
            (
                DragKind::Move,
                Command::MoveClip {
                    clip_id,
                    new_start_ms,
                },
            ) => {
                let by_start = snap_edge(
                    project,
                    clip_id,
                    new_start_ms,
                    playhead_ms,
                    view_width_px,
                    scale,
                );
                let snapped_end = snap_edge(
                    project,
                    clip_id,
                    new_start_ms + self.orig_duration_ms,
                    playhead_ms,
                    view_width_px,
                    scale,
                );
                let by_end = snapped_end - self.orig_duration_ms;
                // 吸着した端だけを候補にし、両端とも吸着したら調整量の
                // 小さい方を採る。吸着しなかった端は「調整量 0」として
                // 競わせない（それだと吸着した側が常に負ける）。
                let start_snapped = by_start != new_start_ms;
                let end_snapped = snapped_end != new_start_ms + self.orig_duration_ms;
                let chosen = match (start_snapped, end_snapped) {
                    (true, true) => {
                        if (by_end - new_start_ms).abs() < (by_start - new_start_ms).abs() {
                            by_end
                        } else {
                            by_start
                        }
                    }
                    (true, false) => by_start,
                    (false, true) => by_end,
                    (false, false) => new_start_ms,
                };
                Command::MoveClip {
                    clip_id,
                    new_start_ms: chosen,
                }
            }
            (
                DragKind::TrimLeft,
                Command::TrimClip {
                    clip_id,
                    new_start_ms,
                    ..
                },
            ) => {
                let start = snap_edge(
                    project,
                    clip_id,
                    new_start_ms,
                    playhead_ms,
                    view_width_px,
                    scale,
                );
                Command::TrimClip {
                    clip_id,
                    new_start_ms: start,
                    new_duration_ms: self.orig_start_ms + self.orig_duration_ms - start,
                }
            }
            (
                DragKind::TrimRight,
                Command::TrimClip {
                    clip_id,
                    new_start_ms,
                    new_duration_ms,
                },
            ) => {
                let end = snap_edge(
                    project,
                    clip_id,
                    new_start_ms + new_duration_ms,
                    playhead_ms,
                    view_width_px,
                    scale,
                );
                Command::TrimClip {
                    clip_id,
                    new_start_ms,
                    new_duration_ms: end - new_start_ms,
                }
            }
            (_, other) => other,
        }
    }
}

/// スナップのしきい値（画面上のピクセル、Issue #45）。
pub const SNAP_THRESHOLD_PX: f32 = 8.0;
/// スナップのグリッド間隔（ms、Issue #45）。
pub const SNAP_GRID_MS: i64 = 1_000;

/// 端の時刻を最も近い吸着先へ寄せる（Issue #45）。吸着先は、対象
/// クリップ以外の全クリップの開始端・終端、再生ヘッド、0ms、
/// 1秒グリッド。しきい値（8px を px_per_ms で換算）以内に吸着先が
/// なければ元の時刻をそのまま返す。同距離の候補が複数ある場合は
/// 候補の列挙順（クリップ端→再生ヘッド→0→グリッド）で先のものを
/// 採る決定的な規則にする。
pub fn snap_edge(
    project: &Project,
    exclude_clip_id: u64,
    edge_ms: i64,
    playhead_ms: i64,
    view_width_px: f32,
    scale: &timeline::ViewScale,
) -> i64 {
    let ppm = scale.px_per_ms(view_width_px);
    let mut targets: Vec<i64> = Vec::new();
    for track in &project.tracks {
        for clip in &track.clips {
            if clip.id == exclude_clip_id {
                continue;
            }
            targets.push(clip.start_ms);
            targets.push(clip.end_ms());
        }
    }
    targets.push(playhead_ms);
    targets.push(0);
    // グリッドは端の前後の目盛りだけを候補にする。
    let grid_below = edge_ms.div_euclid(SNAP_GRID_MS) * SNAP_GRID_MS;
    targets.push(grid_below);
    targets.push(grid_below + SNAP_GRID_MS);

    let mut best: Option<(f32, i64)> = None;
    for target in targets {
        let dist_px = ((target - edge_ms) as f32 * ppm).abs();
        if dist_px <= SNAP_THRESHOLD_PX && best.is_none_or(|(d, _)| dist_px < d) {
            best = Some((dist_px, target));
        }
    }
    best.map(|(_, t)| t).unwrap_or(edge_ms)
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

/// タイトル編集セッション（Issue #50）。リネーム（#57）と同じ
/// インライン編集の意味論で、対象がクリップで確定コマンドが
/// SetClipTitle になる。入力は英数字キーと貼り付けのみ（イシューの
/// スコープ）: ASCII 英数字と空白・一部記号だけを受け付け、
/// 日本語などの文字は取り込まない。
#[derive(Debug, Clone, PartialEq)]
pub struct TitleSession {
    pub clip_id: u64,
    pub text: String,
}

impl TitleSession {
    pub fn begin(clip_id: u64, current_title: &str) -> TitleSession {
        TitleSession {
            clip_id,
            text: current_title.to_string(),
        }
    }

    pub fn handle(&mut self, input: RenameInput) -> RenameAction {
        match input {
            RenameInput::Char(c) => {
                if c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                    self.text.push(c);
                }
                RenameAction::Continue
            }
            RenameInput::Backspace => {
                self.text.pop();
                RenameAction::Continue
            }
            RenameInput::Enter => RenameAction::Commit(Command::SetClipTitle {
                clip_id: self.clip_id,
                title: self.text.clone(),
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
    fn title_session_enter_commits_and_esc_cancels() {
        // Issue #50: タイトル編集は Enter で SetClipTitle を発行し、
        // Esc で中止する。入力は英数字のみ（日本語は取り込まない）。
        let p = sample_project();
        let clip_id = p.tracks[0].clips[0].id;
        let mut session = TitleSession::begin(clip_id, "");
        for c in "Op3n ing".chars() {
            assert_eq!(session.handle(RenameInput::Char(c)), RenameAction::Continue);
        }
        assert_eq!(session.text, "Op3n ing");
        // 日本語・制御文字は取り込まない（イシューのスコープ）
        session.handle(RenameInput::Char('あ'));
        session.handle(RenameInput::Char('\r'));
        assert_eq!(session.text, "Op3n ing");
        assert_eq!(
            session.handle(RenameInput::Backspace),
            RenameAction::Continue
        );
        assert_eq!(session.text, "Op3n in");
        match session.handle(RenameInput::Enter) {
            RenameAction::Commit(cmd) => assert_eq!(
                cmd,
                Command::SetClipTitle {
                    clip_id,
                    title: "Op3n in".into()
                }
            ),
            other => panic!("expected Commit, got {other:?}"),
        }
        let mut session = TitleSession::begin(clip_id, "old");
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

    // Issue #44: 複製・コピー＆ペーストの配置計画とキー追跡。

    fn project_with_tuned_clip() -> (Project, u64, u64) {
        let mut project = Project::new("test44");
        let track = apply(
            &mut project,
            &Command::AddTrack {
                name: "V1".into(),
                kind: TrackKind::Video,
            },
        )
        .unwrap()
        .unwrap();
        let clip = apply(
            &mut project,
            &Command::AddClip {
                track_id: track,
                name: "opening".into(),
                start_ms: 1_000,
                duration_ms: 2_000,
                source: "demo/opening.mp4".into(),
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
        apply(
            &mut project,
            &Command::SetClipFade {
                clip_id: clip,
                fade_in_ms: 300,
                fade_out_ms: 400,
            },
        )
        .unwrap();
        apply(
            &mut project,
            &Command::SetClipTitle {
                clip_id: clip,
                title: "Tuned Title".into(),
            },
        )
        .unwrap();
        (project, track, clip)
    }

    #[test]
    fn duplicate_command_copies_all_clip_parameters() {
        // Issue #44（イシュー指定のテスト）: 複製は元クリップの直後に
        // 置かれ、全パラメータ（cutout を含む）を引き継ぐ。ここでの
        // 「全パラメータ」は設定コマンドで復元できる gain / pitch /
        // cutout / color / fade / title（#50 で追加）と offset
        // （#63 で追加。このテストの元クリップは offset 0 で、
        // offset > 0 の引き継ぎは duplicate_command_copies_clip_offset
        // が担保する）。
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
                assert_eq!(name, "opening");
                assert_eq!(*start_ms, 3_000, "元の終端に接する位置");
                assert_eq!(*duration_ms, 2_000);
                assert_eq!(source, "demo/opening.mp4");
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
        assert_eq!(dup.fade_in_ms, src.fade_in_ms);
        assert_eq!(dup.fade_out_ms, src.fade_out_ms);
        assert_eq!(dup.title_text, src.title_text);
        assert_eq!(dup.offset_ms, src.offset_ms);
        assert_eq!(dup.duration_ms, src.duration_ms);
    }

    #[test]
    fn duplicate_command_copies_clip_offset() {
        // Issue #63: 分割した右クリップ（offset_ms > 0）を複製・
        // 貼り付けしても、コピー側の offset_ms が元と一致する
        // （コアの方式は #62 の SetClipOffset に従う）。
        let (mut project, track, clip_id) = project_with_tuned_clip();
        // クリップ（start 1000 / duration 2000）を 2000 で分割し、
        // offset 1000 の右クリップを得る。
        let right_id = apply(
            &mut project,
            &Command::SplitClip {
                clip_id,
                at_ms: 2_000,
            },
        )
        .unwrap()
        .unwrap();
        let (_, right) = project.clip(right_id).unwrap();
        assert_eq!(right.offset_ms, 1_000);
        // 複製: 配置計画と追随コマンドを適用すると offset が一致する。
        let placement = duplicate_placement(&project, right_id).expect("clip exists");
        let dup_id = apply(&mut project, &placement.add).unwrap().unwrap();
        for cmd in placement.follow_up_commands(dup_id) {
            apply(&mut project, &cmd).unwrap();
        }
        let (_, dup) = project.clip(dup_id).unwrap();
        assert_eq!(dup.offset_ms, 1_000, "duplicate must keep the offset");
        assert_eq!(dup.start_ms, 3_000, "元の右クリップの終端に接する");
        // 貼り付け: クリップボード経由でも offset の追随コマンドが出る。
        let (_, right) = project.clip(right_id).unwrap();
        let paste = paste_placement(right, track, 6_000);
        assert!(paste
            .follow_up_commands(42)
            .contains(&Command::SetClipOffset {
                clip_id: 42,
                offset_ms: 1_000,
            }));
    }

    #[test]
    fn duplicate_placement_unknown_clip_is_none() {
        let (project, _, _) = project_with_tuned_clip();
        assert!(duplicate_placement(&project, 999).is_none());
    }

    #[test]
    fn paste_placement_puts_clip_at_playhead_on_target_track() {
        // Issue #44: 貼り付けは指定トラックの再生ヘッド位置に、
        // コピー元のパラメータで置かれる。実際に適用して、結果の
        // クリップの全パラメータがコピー元と一致することまで見る
        // （レビュー指摘: 追随コマンドの有無だけでは弱い）。
        let (mut project, track, clip_id) = project_with_tuned_clip();
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
        let new_id = apply(&mut project, &placement.add).unwrap().unwrap();
        for cmd in placement.follow_up_commands(new_id) {
            apply(&mut project, &cmd).unwrap();
        }
        let (_, src) = project.clip(clip_id).unwrap();
        let (_, pasted) = project.clip(new_id).unwrap();
        assert_eq!(pasted.start_ms, 4_200);
        assert_eq!(pasted.duration_ms, src.duration_ms);
        assert_eq!(pasted.source, src.source);
        assert_eq!(pasted.gain, src.gain);
        assert_eq!(pasted.pitch_semitones, src.pitch_semitones);
        assert_eq!(pasted.cutout, src.cutout);
        assert_eq!(pasted.color, src.color);
        assert_eq!(pasted.fade_in_ms, src.fade_in_ms);
        assert_eq!(pasted.fade_out_ms, src.fade_out_ms);
    }

    #[test]
    fn placement_application_failure_leaves_state_and_history_unchanged() {
        // Issue #44 レビュー round 2 対応: 配置が失敗する場合
        // （存在しないトラックへの貼り付け）、プロジェクトも履歴も
        // 変わらない。試行が複製の上で行われることの担保。
        let (project, _track, clip_id) = project_with_tuned_clip();
        let mut history = studio_core::History::new(project);
        let before = history.project().dump_json();
        let (_, clip) = history.project().clip(clip_id).unwrap();
        let placement = paste_placement(clip, 999, 4_200);
        assert!(apply_placement(&mut history, &placement).is_err());
        assert_eq!(history.project().dump_json(), before);
        assert!(!history.can_undo());
    }

    #[test]
    fn placement_application_is_a_single_undo_step() {
        // 適用は履歴に1件だけ積まれ、1回の undo で複製前の状態へ
        // 完全に戻る。コマンドごとに履歴へ積むと、途中の undo で
        // 半端なパラメータの複製が残ってしまう。
        let (project, _track, clip_id) = project_with_tuned_clip();
        let mut history = studio_core::History::new(project);
        let before = history.project().dump_json();
        let placement = duplicate_placement(history.project(), clip_id).expect("clip exists");
        let new_id = apply_placement(&mut history, &placement).expect("placement applies");
        let (_, dup) = history.project().clip(new_id).unwrap();
        assert!(dup.cutout, "パラメータ設定まで適用されている");
        assert!(history.can_undo());
        assert!(history.undo(), "1回の undo で戻れる");
        assert_eq!(history.project().dump_json(), before);
        assert!(!history.can_undo(), "履歴は1件だけ");
        assert!(history.redo());
        assert!(history.project().clip(new_id).is_some());
    }

    #[test]
    fn edit_key_tracker_fires_on_ctrl_edges_only() {
        // Issue #44: Ctrl+D/C/V は押下エッジでのみ発火し、押しっぱなしや
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
        // 編集中の押下エッジは発火せず、編集を終えても持ち越さない
        assert_eq!(tracker.update(true, true, true, false, false), None);
        assert_eq!(tracker.update(false, true, true, false, false), None);
    }

    #[test]
    fn x_to_ms_round_trips_at_any_zoom() {
        // Issue #46（イシュー指定のテスト）: 任意の倍率・オフセットで
        // ms→px→ms がピクセル量子化の誤差以内で往復する。
        for zoom in [0.25f32, 0.5, 1.0, 2.0, 8.0] {
            for offset in [0i64, 1_000, 5_000] {
                let scale = timeline::ViewScale {
                    zoom,
                    offset_ms: offset,
                };
                let tolerance = (1.0 / scale.px_per_ms(960.0)).ceil() as i64 + 1;
                for ms in [offset, offset + 500, offset + 1_234, offset + 9_999] {
                    let x = scale.time_to_x(ms, 960.0);
                    let back = x_to_ms(x, 960.0, &scale);
                    assert!(
                        (back - ms).abs() <= tolerance,
                        "zoom={zoom} offset={offset} ms={ms} x={x} back={back}"
                    );
                }
            }
        }
    }

    #[test]
    fn hit_test_uses_view_scale() {
        // Issue #46: ヒットテストは倍率・オフセットを反映する。
        // 既定スケールでは x=130 は opening（1000ms 開始）より左で
        // Empty だが、zoom 2・offset 1000 では opening の本体になる。
        let p = sample_project();
        let id = clip_id(&p);
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 130.0, 48.0),
            HitTarget::Empty { track_index: 0 }
        );
        let zoomed = timeline::ViewScale {
            zoom: 2.0,
            offset_ms: 1_000,
        };
        assert_eq!(
            hit_test(&p, 960.0, &zoomed, 130.0, 48.0),
            HitTarget::ClipBody { clip_id: id }
        );
    }

    #[test]
    fn hit_test_has_no_fade_in_corner_on_clipped_left() {
        // Issue #51 レビュー Minor: 左端が画面外に切り取られた
        // クリップは、上端帯でもフェードインコーナーを持たない
        // （Issue #46 の左端と同じ扱い）。
        let p = sample_project();
        let id = clip_id(&p);
        let scrolled = timeline::ViewScale {
            zoom: 1.0,
            offset_ms: 1_500,
        };
        // opening は 1000..3000、可視部分は x=120..246。上端帯の
        // 左端付近（123, 26）はコーナーではなく本体になる。
        assert_eq!(
            hit_test(&p, 960.0, &scrolled, 123.0, 26.0),
            HitTarget::ClipBody { clip_id: id }
        );
    }

    #[test]
    fn hit_test_ignores_phantom_edge_of_clipped_clip() {
        // Issue #46 レビュー round 2 指摘: スクロールで左端が画面外に
        // 出たクリップは、切り取られた矩形の左端（ヘッダー境界）に
        // 実在しないトリム掴み位置を持たない。見えている部分は本体。
        let p = sample_project();
        let id = clip_id(&p);
        let scrolled = timeline::ViewScale {
            zoom: 1.0,
            offset_ms: 1_500,
        };
        // opening は 1000..3000、可視部分は 1500..3000（x=120..246）
        assert_eq!(
            hit_test(&p, 960.0, &scrolled, 123.0, 48.0),
            HitTarget::ClipBody { clip_id: id },
            "切り取られた左端を掴み位置にしてはいけない"
        );
        // 本物の右端（3000ms → x=246）は従来どおり掴める
        assert_eq!(
            hit_test(&p, 960.0, &scrolled, 246.0, 48.0),
            HitTarget::ClipRightEdge { clip_id: id }
        );
        // 完全に表示範囲より前のクリップは、境界でも何にも当たらない
        let mut p2 = Project::new("t");
        let v = apply(
            &mut p2,
            &Command::AddTrack {
                name: "V1".into(),
                kind: TrackKind::Video,
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p2,
            &Command::AddClip {
                track_id: v,
                name: "past".into(),
                start_ms: 0,
                duration_ms: 500,
                source: "demo/opening.mp4".into(),
            },
        )
        .unwrap();
        assert_eq!(
            hit_test(&p2, 960.0, &scrolled, 120.0, 48.0),
            HitTarget::Empty { track_index: 0 }
        );
    }

    // Issue #53: ドロップ取り込みの配置規則。
    #[test]
    fn drop_position_maps_to_track_and_time() {
        // #30 の調査でドロップ座標は取得できないため、配置先は
        // 「素材種別に合うトラック（選択中を優先）＋再生ヘッド位置」。
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
        let a1 = apply(
            &mut p,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        // 映像ファイル: A1 を選択中でも映像トラックへ
        let cmd =
            drop_placement_command(&p, Some(a1), 1_500, "/tmp/clip.mp4", 2_000, DropKind::Video)
                .unwrap();
        match cmd {
            Command::AddClip {
                track_id,
                name,
                start_ms,
                duration_ms,
                source,
            } => {
                assert_eq!(track_id, v1);
                assert_eq!(name, "clip");
                assert_eq!(start_ms, 1_500);
                assert_eq!(duration_ms, 2_000);
                assert_eq!(source, "/tmp/clip.mp4");
            }
            other => panic!("expected AddClip, got {other:?}"),
        }
        // 音声ファイル: 選択中の A1 へ
        let cmd =
            drop_placement_command(&p, Some(a1), 500, "/x.wav", 700, DropKind::Audio).unwrap();
        match cmd {
            Command::AddClip { track_id, .. } => assert_eq!(track_id, a1),
            other => panic!("expected AddClip, got {other:?}"),
        }
        // 再生ヘッドが負なら 0 に丸める
        let cmd =
            drop_placement_command(&p, Some(v1), -50, "/x.mp4", 700, DropKind::Video).unwrap();
        match cmd {
            Command::AddClip { start_ms, .. } => assert_eq!(start_ms, 0),
            other => panic!("expected AddClip, got {other:?}"),
        }
        // 合う種別のトラックが無い・長さ 0 は配置できない
        let mut audio_only = Project::new("t");
        apply(
            &mut audio_only,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap();
        assert!(
            drop_placement_command(&audio_only, None, 0, "/x.mp4", 500, DropKind::Video).is_none()
        );
        assert!(drop_placement_command(&p, None, 0, "/x.mp4", 0, DropKind::Video).is_none());
    }

    #[test]
    fn drop_kind_classifies_by_extension() {
        // Issue #53: 対応拡張子は media.rs の利用対象（動画 mp4/mov、
        // 画像 png/jpg、音声 wav）。大文字小文字は問わない。
        use std::path::Path;
        assert_eq!(drop_kind(Path::new("/a/b.mp4")), Some(DropKind::Video));
        assert_eq!(drop_kind(Path::new("/a/b.MOV")), Some(DropKind::Video));
        assert_eq!(drop_kind(Path::new("/a/b.png")), Some(DropKind::Image));
        assert_eq!(drop_kind(Path::new("/a/b.jpg")), Some(DropKind::Image));
        assert_eq!(drop_kind(Path::new("/a/b.wav")), Some(DropKind::Audio));
        assert_eq!(drop_kind(Path::new("/a/b.txt")), None);
        assert_eq!(drop_kind(Path::new("/a/b")), None);
    }

    // Issue #45: スナップ（移動・トリムの吸着）。

    fn project_with_neighbor() -> (Project, u64, u64) {
        // V1: opening 1000..3000（ドラッグ対象）、interview 6000..8000（隣）
        let mut p = sample_project();
        let track_id = p.tracks[0].id;
        let neighbor = apply(
            &mut p,
            &Command::AddClip {
                track_id,
                name: "interview".into(),
                start_ms: 6_000,
                duration_ms: 2_000,
                source: "demo/interview.mp4".into(),
            },
        )
        .unwrap()
        .unwrap();
        let opening = clip_id(&p);
        (p, opening, neighbor)
    }

    #[test]
    fn snap_picks_nearest_clip_edge_within_threshold() {
        // Issue #45（イシュー指定のテスト）: しきい値（8px ≒ 95ms @960px）
        // 内では最も近いクリップ端に吸着し、外では元の位置のまま。
        let (p, opening, _neighbor) = project_with_neighbor();
        // 隣の開始端 6000 に 50ms まで近づいた端は 6000 に吸着
        assert_eq!(
            snap_edge(
                &p,
                opening,
                5_950,
                0,
                960.0,
                &timeline::ViewScale::default()
            ),
            6_000
        );
        // 隣の終端 8000 にも吸着
        assert_eq!(
            snap_edge(
                &p,
                opening,
                7_960,
                0,
                960.0,
                &timeline::ViewScale::default()
            ),
            8_000
        );
        // どこにも近くない端はそのまま
        assert_eq!(
            snap_edge(
                &p,
                opening,
                5_500,
                0,
                960.0,
                &timeline::ViewScale::default()
            ),
            5_500
        );
    }

    #[test]
    fn snap_excludes_the_dragged_clips_own_edges() {
        // Issue #45: ドラッグ中クリップ自身の端は吸着先に含めない。
        // 自分の開始端 1500（グリッド外）に 40ms まで近づいても、
        // 他に吸着先がなければそのままになる。
        let mut p = sample_project();
        let track_id = p.tracks[0].id;
        let odd = apply(
            &mut p,
            &Command::AddClip {
                track_id,
                name: "odd".into(),
                start_ms: 1_500,
                duration_ms: 1_100,
                source: "demo/odd.mp4".into(),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            snap_edge(
                &p,
                odd,
                1_540,
                9_000,
                960.0,
                &timeline::ViewScale::default()
            ),
            1_540
        );
    }

    #[test]
    fn snap_targets_include_playhead_zero_and_grid() {
        // Issue #45: 吸着先は他クリップ端・再生ヘッド・0ms・1秒グリッド。
        let (p, opening, _neighbor) = project_with_neighbor();
        // 再生ヘッド 4200 に吸着
        assert_eq!(
            snap_edge(
                &p,
                opening,
                4_160,
                4_200,
                960.0,
                &timeline::ViewScale::default()
            ),
            4_200
        );
        // 0ms に吸着
        assert_eq!(
            snap_edge(
                &p,
                opening,
                60,
                4_200,
                960.0,
                &timeline::ViewScale::default()
            ),
            0
        );
        // 1秒グリッド 4000 に吸着（再生ヘッドは遠い）
        assert_eq!(
            snap_edge(
                &p,
                opening,
                3_950,
                9_000,
                960.0,
                &timeline::ViewScale::default()
            ),
            4_000
        );
        // 最も近いものを選ぶ: グリッド 4000（30ms）と再生ヘッド 4080（50ms）ならグリッド
        assert_eq!(
            snap_edge(
                &p,
                opening,
                4_030,
                4_080,
                960.0,
                &timeline::ViewScale::default()
            ),
            4_000
        );
    }

    #[test]
    fn drag_move_snaps_end_to_neighbor_start() {
        // Issue #45: 移動でクリップの終端が隣の開始端に吸着し、
        // 隙間なく接する位置で止まる。
        let (p, opening, _neighbor) = project_with_neighbor();
        let drag = Drag::begin(&p, &HitTarget::ClipBody { clip_id: opening }, 1_500).unwrap();
        // ポインタを +4450ms 動かす: 候補の終端は 7450 ではなく…
        // 開始 1000+4450=5450、終端 7450。吸着先は隣の終端 8000 でも
        // 開始 6000 でもないため、そのまま。
        let cmd =
            drag.snapped_command_for(&p, 5_950, 0, 960.0, &timeline::ViewScale::default(), true);
        assert_eq!(
            cmd,
            Command::MoveClip {
                clip_id: opening,
                new_start_ms: 5_450
            }
        );
        // 終端が隣の開始（6000）に 50ms まで近づく移動: 開始候補 3950、
        // 終端候補 5950 → 終端が吸着して開始は 4000 になる。
        let cmd =
            drag.snapped_command_for(&p, 4_450, 0, 960.0, &timeline::ViewScale::default(), true);
        assert_eq!(
            cmd,
            Command::MoveClip {
                clip_id: opening,
                new_start_ms: 4_000
            }
        );
        // Alt（スナップ無効）では候補がそのまま返る
        let cmd =
            drag.snapped_command_for(&p, 4_450, 0, 960.0, &timeline::ViewScale::default(), false);
        assert_eq!(
            cmd,
            Command::MoveClip {
                clip_id: opening,
                new_start_ms: 3_950
            }
        );
    }

    #[test]
    fn drag_trim_snaps_edges() {
        // Issue #45: トリムでも同じ吸着が効く。
        let (p, opening, _neighbor) = project_with_neighbor();
        // 右端トリム: 終端候補 5950 → 隣の開始 6000 に吸着し、
        // duration は 1000..6000 の 5000 になる。
        let drag = Drag::begin(&p, &HitTarget::ClipRightEdge { clip_id: opening }, 3_000).unwrap();
        let cmd =
            drag.snapped_command_for(&p, 5_950, 0, 960.0, &timeline::ViewScale::default(), true);
        assert_eq!(
            cmd,
            Command::TrimClip {
                clip_id: opening,
                new_start_ms: 1_000,
                new_duration_ms: 5_000
            }
        );
        // 左端トリム: 開始候補 1050 → 1秒グリッド 1000 に吸着し、
        // 終端 3000 は保たれる。
        let drag = Drag::begin(&p, &HitTarget::ClipLeftEdge { clip_id: opening }, 1_000).unwrap();
        let cmd =
            drag.snapped_command_for(&p, 1_050, 0, 960.0, &timeline::ViewScale::default(), true);
        assert_eq!(
            cmd,
            Command::TrimClip {
                clip_id: opening,
                new_start_ms: 1_000,
                new_duration_ms: 2_000
            }
        );
        // 左端トリムで再生ヘッド（500）へ吸着
        let cmd =
            drag.snapped_command_for(&p, 540, 500, 960.0, &timeline::ViewScale::default(), true);
        assert_eq!(
            cmd,
            Command::TrimClip {
                clip_id: opening,
                new_start_ms: 500,
                new_duration_ms: 2_500
            }
        );
    }
    #[test]
    fn drag_move_snaps_end_even_when_start_has_no_target() {
        // Issue #45 の実装中に実アプリで見つけた不具合の回帰テスト:
        // 開始端に吸着先がなく終端だけが近い場合でも、終端の吸着が
        // 採用されなければならない（旧実装は「開始端の調整距離 0」が
        // 常に勝ち、終端の吸着が捨てられた）。
        let mut p = Project::new("demo");
        let v = apply(
            &mut p,
            &Command::AddTrack {
                name: "V1".into(),
                kind: TrackKind::Video,
            },
        )
        .unwrap()
        .unwrap();
        let opening = apply(
            &mut p,
            &Command::AddClip {
                track_id: v,
                name: "opening".into(),
                start_ms: 0,
                duration_ms: 2_000,
                source: "demo/opening.mp4".into(),
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::AddClip {
                track_id: v,
                name: "interview".into(),
                start_ms: 2_500,
                duration_ms: 4_000,
                source: "demo/interview.mp4".into(),
            },
        )
        .unwrap();
        let drag = Drag::begin(&p, &HitTarget::ClipBody { clip_id: opening }, 1_000).unwrap();
        // 候補: 開始 452（吸着先なし）、終端 2452（interview の開始
        // 2500 に 48ms まで接近）→ 終端が吸着して開始は 500。
        let cmd =
            drag.snapped_command_for(&p, 1_452, 300, 960.0, &timeline::ViewScale::default(), true);
        assert_eq!(
            cmd,
            Command::MoveClip {
                clip_id: opening,
                new_start_ms: 500
            }
        );
    }

    // Issue #51: ゲインスライダーとフェードのコーナードラッグ。
    #[test]
    fn gain_slider_maps_to_set_clip_gain() {
        // スライダーの割合 0..=1 を ゲイン 0.0..=2.0（中央が等倍）に
        // 対応させ、SetClipGain コマンドを生成する。範囲外は丸める。
        match gain_slider_command(7, 0.0) {
            Command::SetClipGain { clip_id, gain } => {
                assert_eq!(clip_id, 7);
                assert_eq!(gain, 0.0);
            }
            other => panic!("expected SetClipGain, got {other:?}"),
        }
        match gain_slider_command(7, 0.5) {
            Command::SetClipGain { gain, .. } => assert_eq!(gain, 1.0),
            other => panic!("expected SetClipGain, got {other:?}"),
        }
        match gain_slider_command(7, 1.0) {
            Command::SetClipGain { gain, .. } => assert_eq!(gain, 2.0),
            other => panic!("expected SetClipGain, got {other:?}"),
        }
        match gain_slider_command(7, 1.4) {
            Command::SetClipGain { gain, .. } => assert_eq!(gain, 2.0),
            other => panic!("expected SetClipGain, got {other:?}"),
        }
        match gain_slider_command(7, -0.2) {
            Command::SetClipGain { gain, .. } => assert_eq!(gain, 0.0),
            other => panic!("expected SetClipGain, got {other:?}"),
        }
    }

    #[test]
    fn hit_test_distinguishes_fade_corners_from_trim_edges() {
        // opening の矩形: x = 204, w = 168, y = 24..72（幅 960）。
        // 上端帯（12px）の左右コーナーはフェード、帯より下の端は
        // 従来どおりトリムになる（DAW #34 と同じ規則）。
        let p = sample_project();
        let id = clip_id(&p);
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 206.0, 26.0),
            HitTarget::ClipFadeInCorner { clip_id: id }
        );
        assert_eq!(
            hit_test(&p, 960.0, &timeline::ViewScale::default(), 370.0, 26.0),
            HitTarget::ClipFadeOutCorner { clip_id: id }
        );
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
    fn drag_fade_corners_build_set_clip_fade() {
        // コーナードラッグは対象のフェードだけを 0..=duration に
        // クランプして変える（DAW #34 と同じ計算）。
        let mut p = sample_project();
        let id = clip_id(&p);
        apply(
            &mut p,
            &Command::SetClipFade {
                clip_id: id,
                fade_in_ms: 200,
                fade_out_ms: 300,
            },
        )
        .unwrap();
        // フェードイン: 右へ 100ms 動かすと 200 → 300
        let drag = Drag::begin(&p, &HitTarget::ClipFadeInCorner { clip_id: id }, 1_000).unwrap();
        match drag.command_for(1_100) {
            Command::SetClipFade {
                fade_in_ms,
                fade_out_ms,
                ..
            } => {
                assert_eq!(fade_in_ms, 300);
                assert_eq!(fade_out_ms, 300);
            }
            other => panic!("expected SetClipFade, got {other:?}"),
        }
        // フェードアウト: 右へ 100ms 動かすと 300 → 200
        let drag = Drag::begin(&p, &HitTarget::ClipFadeOutCorner { clip_id: id }, 3_000).unwrap();
        match drag.command_for(3_100) {
            Command::SetClipFade {
                fade_in_ms,
                fade_out_ms,
                ..
            } => {
                assert_eq!(fade_in_ms, 200);
                assert_eq!(fade_out_ms, 200);
            }
            other => panic!("expected SetClipFade, got {other:?}"),
        }
        // 大きく動かしても duration（2000）でクランプされる
        let drag = Drag::begin(&p, &HitTarget::ClipFadeInCorner { clip_id: id }, 1_000).unwrap();
        match drag.command_for(9_000) {
            Command::SetClipFade { fade_in_ms, .. } => assert_eq!(fade_in_ms, 2_000),
            other => panic!("expected SetClipFade, got {other:?}"),
        }
    }
}
