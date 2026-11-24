//! コマンド層：プロジェクト状態への唯一の変更経路（ADR-0004）。
//!
//! GUI・HTTP API・MCP サーバーはすべて、ここで定義したコマンドを
//! JSON で受け取り [`apply`] で適用する。コマンドは検証してから適用され、
//! 失敗したコマンドは状態を変えない。

use serde::{Deserialize, Serialize};

use crate::model::{
    Clip, ColorTag, Instrument, Note, Project, ScriptEffect, Track, TrackKind, TransitionKind,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    AddTrack {
        name: String,
        kind: TrackKind,
    },
    RemoveTrack {
        track_id: u64,
    },
    /// 既存トラックの名前を変更する（Issue #55）。
    /// 空白のみの名前は受け付けない。
    RenameTrack {
        track_id: u64,
        name: String,
    },
    AddClip {
        track_id: u64,
        name: String,
        start_ms: i64,
        duration_ms: i64,
        source: String,
    },
    MoveClip {
        clip_id: u64,
        new_start_ms: i64,
    },
    /// クリップの開始位置と長さをまとめて変える（トリミング）。
    /// 左端のドラッグは end を保った (start, duration) の組として、
    /// 右端のドラッグは start を保った組として、このコマンドに落ちる。
    TrimClip {
        clip_id: u64,
        new_start_ms: i64,
        new_duration_ms: i64,
    },
    /// クリップを指定時刻で2つに分割する（Issue #32）。
    /// 左クリップは元の id のまま短くなり、右クリップは新規 id で、
    /// `offset_ms` が分割点まで進む。分割位置はクリップ内部
    /// （start < at_ms < end）でのみ有効。
    SplitClip {
        clip_id: u64,
        at_ms: i64,
    },
    RemoveClip {
        clip_id: u64,
    },
    SetClipGain {
        clip_id: u64,
        gain: f32,
    },
    /// クリップの素材内開始位置（offset_ms）を設定する（Issue #62）。
    /// 0 = 素材の先頭から。負の値は受け付けない。素材の長さはコアでは
    /// 分からないため、上限は検証しない（範囲外は再生側で無音／
    /// フレームなしに倒れる）。
    SetClipOffset {
        clip_id: u64,
        offset_ms: i64,
    },
    /// クリップのタイトル文字列を設定する（Issue #50）。
    /// 空文字はタイトルなし（消去）。
    SetClipTitle {
        clip_id: u64,
        title: String,
    },
    /// クリップの Lua スクリプトエフェクトを設定する（Issue #81）。
    /// None は解除。スクリプト参照が空白のみの場合は受け付けない。
    SetClipScriptEffect {
        clip_id: u64,
        effect: Option<ScriptEffect>,
    },
    /// クリップの先頭境界のトランジションを設定する
    /// （Issue #49、ADR-0014）。検証は種類で分ける:
    /// cross_dissolve は 0 < duration_ms <= クリップ長、
    /// none は解除で duration_ms = 0 のみ許可する。
    SetClipTransition {
        clip_id: u64,
        kind: TransitionKind,
        duration_ms: i64,
    },
    SetTrackVolume {
        track_id: u64,
        volume: f32,
    },
    SetTrackPan {
        track_id: u64,
        pan: f32,
    },
    /// トラックのミュートを設定する（Issue #39）。
    SetTrackMute {
        track_id: u64,
        muted: bool,
    },
    /// トラックのソロを設定する（Issue #39）。複数ソロ可。
    SetTrackSolo {
        track_id: u64,
        solo: bool,
    },
    SetClipCutout {
        clip_id: u64,
        enabled: bool,
    },
    SetClipPitch {
        clip_id: u64,
        semitones: f32,
    },
    /// クリップのフェードイン／フェードアウトの長さを設定する
    /// （Issue #34）。どちらも 0 以上、クリップの長さ以下。
    /// 包絡は直線のみ。
    SetClipFade {
        clip_id: u64,
        fade_in_ms: i64,
        fade_out_ms: i64,
    },
    /// プロジェクトのテンポ（BPM）を設定する（Issue #35）。
    /// 有効範囲は 20..=300。
    SetBpm {
        bpm: f32,
    },
    /// トラックの色を設定する（Issue #58、ADR-0013）。
    /// None は「なし（既定の種別色）」に戻す。
    SetTrackColor {
        track_id: u64,
        color: Option<ColorTag>,
    },
    /// クリップの色を設定する（Issue #58、ADR-0013）。
    /// None は「なし（既定の種別色）」に戻す。
    SetClipColor {
        clip_id: u64,
        color: Option<ColorTag>,
    },
    /// トラックの音色（楽器）を設定する（Issue #68、ADR-0016）。
    /// None は楽器なしに戻す。直接設定すると音色参照（tone_id）は
    /// 外れる（Issue #73、ADR-0018）。
    SetTrackInstrument {
        track_id: u64,
        instrument: Option<Instrument>,
    },
    /// 音色をプロジェクトに追加する（Issue #73、ADR-0018）。
    /// 戻り値は新しい音色の id。
    AddTone {
        name: String,
        instrument: Instrument,
    },
    /// 音色の名前を変える（Issue #73）。
    RenameTone {
        tone_id: u64,
        name: String,
    },
    /// 音色の中身（楽器）を差し替える（Issue #73）。割り当て中の
    /// 全トラックに即座に反映される（参照で持つため）。
    UpdateTone {
        tone_id: u64,
        instrument: Instrument,
    },
    /// 音色を削除する（Issue #73）。参照しているトラックの
    /// tone_id は外れる（直接の楽器設定に戻る）。
    RemoveTone {
        tone_id: u64,
    },
    /// トラックへ音色を割り当てる／外す（Issue #73）。
    SetTrackTone {
        track_id: u64,
        tone_id: Option<u64>,
    },
    /// クリップにノートを1つ追加する（Issue #68、ADR-0016）。
    /// ノートはクリップの範囲内に収まること（start_ms >= 0、
    /// duration_ms > 0、終端 <= クリップ長）が不変条件で、
    /// pitch・velocity は 0..=127。
    AddNote {
        clip_id: u64,
        pitch: u8,
        start_ms: i64,
        duration_ms: i64,
        velocity: u8,
    },
    /// ノートを移動する（音高と開始位置。長さは変えない、Issue #69）
    MoveNote {
        clip_id: u64,
        note_index: usize,
        new_pitch: u8,
        new_start_ms: i64,
    },
    /// ノートを1つ削除する（Issue #69）
    RemoveNote {
        clip_id: u64,
        note_index: usize,
    },
    /// テスト専用（Issue #61）：状態を変更してから失敗するコマンド。
    /// 実コマンドは全て「検証 → 変更」の順で失敗時に状態を変えないため、
    /// `History::apply_transient` の失敗時復元を再現するにはこの
    /// コマンドが要る。本番ビルドには含まれない。
    #[cfg(test)]
    MutateThenFail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    TrackNotFound(u64),
    ClipNotFound(u64),
    ToneNotFound(u64),
    InvalidRange(&'static str),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::TrackNotFound(id) => write!(f, "track {id} not found"),
            CommandError::ClipNotFound(id) => write!(f, "clip {id} not found"),
            CommandError::ToneNotFound(id) => write!(f, "tone {id} not found"),
            CommandError::InvalidRange(msg) => write!(f, "invalid range: {msg}"),
        }
    }
}

impl std::error::Error for CommandError {}

/// 楽器の不変条件（ADR-0016 / ADR-0017）。トラックへの直接設定と
/// 音色への格納で共有する。
fn validate_instrument(instrument: &Instrument) -> Result<(), CommandError> {
    // サンプラーの root_note は素材の原音の MIDI ノート番号（0–127）。
    if let Instrument::Sampler { root_note, .. } = instrument {
        if *root_note > 127 {
            return Err(CommandError::InvalidRange(
                "root_note must be within 0..=127",
            ));
        }
    }
    // プラグイン楽器: プラグインへの参照が空では鳴らし先が無い。
    // プリセットは None（既定状態）か非空のパスのどちらか。
    if let Instrument::Plugin {
        plugin_path,
        preset_path,
        ..
    } = instrument
    {
        if plugin_path.trim().is_empty() {
            return Err(CommandError::InvalidRange("plugin_path must not be empty"));
        }
        if let Some(preset) = preset_path {
            if preset.trim().is_empty() {
                return Err(CommandError::InvalidRange("preset_path must not be empty"));
            }
        }
    }
    Ok(())
}

/// 音色名の不変条件（Issue #73）: 空白のみの名前は付けられない。
fn validate_tone_name(name: &str) -> Result<(), CommandError> {
    if name.trim().is_empty() {
        return Err(CommandError::InvalidRange("tone name must not be empty"));
    }
    Ok(())
}

/// コマンドを適用する。成功時は新しく作られた要素の id（該当する場合）を返す。
/// 失敗時は状態を一切変えない。
pub fn apply(project: &mut Project, command: &Command) -> Result<Option<u64>, CommandError> {
    match command {
        Command::SetBpm { bpm } => {
            if !bpm.is_finite() || *bpm < 20.0 || *bpm > 300.0 {
                return Err(CommandError::InvalidRange("bpm must be within 20..=300"));
            }
            project.bpm = *bpm;
            Ok(None)
        }
        Command::AddTrack { name, kind } => {
            let id = project.alloc_id();
            project.tracks.push(Track {
                id,
                name: name.clone(),
                kind: *kind,
                clips: Vec::new(),
                volume: 1.0,
                pan: 0.0,
                color: None,
                muted: false,
                solo: false,
                instrument: None,
                tone_id: None,
            });
            Ok(Some(id))
        }
        Command::RemoveTrack { track_id } => {
            let pos = project
                .tracks
                .iter()
                .position(|t| t.id == *track_id)
                .ok_or(CommandError::TrackNotFound(*track_id))?;
            project.tracks.remove(pos);
            Ok(None)
        }
        Command::RenameTrack { track_id, name } => {
            if name.trim().is_empty() {
                return Err(CommandError::InvalidRange("name must not be blank"));
            }
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.id == *track_id)
                .ok_or(CommandError::TrackNotFound(*track_id))?;
            track.name = name.clone();
            Ok(None)
        }
        Command::AddClip {
            track_id,
            name,
            start_ms,
            duration_ms,
            source,
        } => {
            if *start_ms < 0 {
                return Err(CommandError::InvalidRange("start_ms must be >= 0"));
            }
            if *duration_ms <= 0 {
                return Err(CommandError::InvalidRange("duration_ms must be > 0"));
            }
            if project.track(*track_id).is_none() {
                return Err(CommandError::TrackNotFound(*track_id));
            }
            let id = project.alloc_id();
            project
                .track_mut(*track_id)
                .expect("checked above")
                .clips
                .push(Clip {
                    id,
                    name: name.clone(),
                    start_ms: *start_ms,
                    duration_ms: *duration_ms,
                    source: source.clone(),
                    offset_ms: 0,
                    gain: 1.0,
                    cutout: false,
                    pitch_semitones: 0.0,
                    color: None,
                    fade_in_ms: 0,
                    fade_out_ms: 0,
                    title_text: String::new(),
                    transition_in_kind: TransitionKind::None,
                    transition_in_ms: 0,
                    notes: Vec::new(),
                    script_effect: None,
                });
            Ok(Some(id))
        }
        Command::MoveClip {
            clip_id,
            new_start_ms,
        } => {
            if *new_start_ms < 0 {
                return Err(CommandError::InvalidRange("new_start_ms must be >= 0"));
            }
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            clip.start_ms = *new_start_ms;
            Ok(None)
        }
        Command::TrimClip {
            clip_id,
            new_start_ms,
            new_duration_ms,
        } => {
            if *new_start_ms < 0 {
                return Err(CommandError::InvalidRange("new_start_ms must be >= 0"));
            }
            if *new_duration_ms <= 0 {
                return Err(CommandError::InvalidRange("new_duration_ms must be > 0"));
            }
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            clip.start_ms = *new_start_ms;
            clip.duration_ms = *new_duration_ms;
            // フェードが新しい長さを超えないよう保つ（Issue #34）。
            clip.fade_in_ms = clip.fade_in_ms.min(*new_duration_ms);
            clip.fade_out_ms = clip.fade_out_ms.min(*new_duration_ms);
            // トランジションも不変条件（<= クリップ長）を保つよう
            // クランプする（Issue #49 レビュー指摘、ADR-0014）。
            clip.transition_in_ms = clip.transition_in_ms.min(*new_duration_ms);
            // ノートも不変条件（クリップ範囲内）を保つ（Issue #68、
            // ADR-0016）: 範囲外に出たノートは削除し、終端が
            // はみ出すノートは収まるようクランプする。
            clip.notes.retain(|n| n.start_ms < *new_duration_ms);
            for note in &mut clip.notes {
                note.duration_ms = note.duration_ms.min(*new_duration_ms - note.start_ms);
            }
            Ok(None)
        }
        Command::SplitClip { clip_id, at_ms } => {
            let (track_idx, clip_idx) = project
                .tracks
                .iter()
                .enumerate()
                .find_map(|(ti, t)| {
                    t.clips
                        .iter()
                        .position(|c| c.id == *clip_id)
                        .map(|ci| (ti, ci))
                })
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = &project.tracks[track_idx].clips[clip_idx];
            if *at_ms <= clip.start_ms || *at_ms >= clip.end_ms() {
                return Err(CommandError::InvalidRange(
                    "split point must be inside the clip",
                ));
            }
            let new_id = project.alloc_id();
            let clip = &project.tracks[track_idx].clips[clip_idx];
            let left_duration = *at_ms - clip.start_ms;
            let right_duration = clip.end_ms() - *at_ms;
            // ノートの振り分け（Issue #68、ADR-0016）: 分割点 rel
            // （クリップ相対）で分ける。境界を跨ぐノートは分割点で
            // 切る（左は rel までで縮み、右は 0 から残りの長さに
            // なる）ため、分割前後で鳴るメロディーは変わらない。
            let rel = left_duration;
            let mut left_notes: Vec<Note> = Vec::new();
            let mut right_notes: Vec<Note> = Vec::new();
            for note in &clip.notes {
                if note.start_ms < rel {
                    left_notes.push(Note {
                        duration_ms: note.end_ms().min(rel) - note.start_ms,
                        ..*note
                    });
                }
                if note.end_ms() > rel {
                    let start = note.start_ms.max(rel);
                    right_notes.push(Note {
                        start_ms: start - rel,
                        duration_ms: note.end_ms() - start,
                        ..*note
                    });
                }
            }
            let right = Clip {
                id: new_id,
                name: clip.name.clone(),
                start_ms: *at_ms,
                duration_ms: right_duration,
                source: clip.source.clone(),
                offset_ms: clip.offset_ms + (*at_ms - clip.start_ms),
                gain: clip.gain,
                cutout: clip.cutout,
                pitch_semitones: clip.pitch_semitones,
                color: clip.color,
                // 分割点は切り口なので、右クリップはフェードインを
                // 引き継がない。フェードアウトは外側の端として引き継ぐ
                // （Issue #34、分割後の長さでクランプ）。
                fade_in_ms: 0,
                fade_out_ms: clip.fade_out_ms.min(right_duration),
                // タイトルはクリップ全体への注釈として右側にも
                // 引き継ぐ（Issue #50）。
                title_text: clip.title_text.clone(),
                // 分割点は新しい境界なので、右クリップは先頭境界の
                // トランジションを引き継がない（Issue #49、ADR-0014。
                // フェードインを付けないのと同じ扱い）。左クリップは
                // 前の境界の属性をそのまま保持する。
                transition_in_kind: TransitionKind::None,
                transition_in_ms: 0,
                notes: right_notes,
                // スクリプトエフェクトはクリップの内容への効果
                // なので、ゲインや切り抜きと同じく右側にも引き継ぐ
                // （Issue #81）。
                script_effect: clip.script_effect.clone(),
            };
            let track = &mut project.tracks[track_idx];
            // 左クリップはフェードインを外側の端として引き継ぎ、
            // フェードアウトは付けない（分割点は切り口）。
            track.clips[clip_idx].fade_in_ms = track.clips[clip_idx].fade_in_ms.min(left_duration);
            track.clips[clip_idx].fade_out_ms = 0;
            // 左クリップが保持するトランジションも、短くなった
            // 長さへクランプして不変条件を保つ（Issue #49、ADR-0014）。
            track.clips[clip_idx].transition_in_ms =
                track.clips[clip_idx].transition_in_ms.min(left_duration);
            track.clips[clip_idx].notes = left_notes;
            track.clips[clip_idx].duration_ms = left_duration;
            track.clips.insert(clip_idx + 1, right);
            Ok(Some(new_id))
        }
        Command::RemoveClip { clip_id } => {
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            track.clips.retain(|c| c.id != *clip_id);
            Ok(None)
        }
        Command::SetClipGain { clip_id, gain } => {
            if !(0.0..=4.0).contains(gain) {
                return Err(CommandError::InvalidRange("gain must be within 0.0..=4.0"));
            }
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            clip.gain = *gain;
            Ok(None)
        }
        Command::SetClipOffset { clip_id, offset_ms } => {
            if *offset_ms < 0 {
                return Err(CommandError::InvalidRange("offset must be >= 0"));
            }
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            clip.offset_ms = *offset_ms;
            Ok(None)
        }
        Command::SetClipTitle { clip_id, title } => {
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            clip.title_text = title.clone();
            Ok(None)
        }
        Command::SetClipScriptEffect { clip_id, effect } => {
            if let Some(effect) = effect {
                if effect.script.trim().is_empty() {
                    return Err(CommandError::InvalidRange("script must not be empty"));
                }
                // 非有限の数値パラメータは JSON で null になり、無タグ
                // enum の復元に失敗してプロジェクト全体が再読込不能に
                // なる。BPM・ピッチの前例どおり有限値だけを許す
                // （Issue #81 レビュー round 2 Major）。
                for value in effect.params.values() {
                    if let crate::model::EffectParam::Number(n) = value {
                        if !n.is_finite() {
                            return Err(CommandError::InvalidRange(
                                "script param numbers must be finite",
                            ));
                        }
                    }
                }
            }
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            clip.script_effect = effect.clone();
            Ok(None)
        }
        Command::SetClipTransition {
            clip_id,
            kind,
            duration_ms,
        } => {
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            match kind {
                TransitionKind::CrossDissolve => {
                    if *duration_ms <= 0 || *duration_ms > clip.duration_ms {
                        return Err(CommandError::InvalidRange(
                            "transition duration must be within 1..=clip duration",
                        ));
                    }
                }
                TransitionKind::None => {
                    if *duration_ms != 0 {
                        return Err(CommandError::InvalidRange(
                            "clearing a transition requires duration_ms = 0",
                        ));
                    }
                }
            }
            clip.transition_in_kind = *kind;
            // none のときは常に 0 に正規化される（検証で 0 のみ許可）
            clip.transition_in_ms = *duration_ms;
            Ok(None)
        }
        Command::SetTrackVolume { track_id, volume } => {
            if !(0.0..=2.0).contains(volume) {
                return Err(CommandError::InvalidRange(
                    "volume must be within 0.0..=2.0",
                ));
            }
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.id == *track_id)
                .ok_or(CommandError::TrackNotFound(*track_id))?;
            track.volume = *volume;
            Ok(None)
        }
        Command::SetTrackPan { track_id, pan } => {
            if !(-1.0..=1.0).contains(pan) {
                return Err(CommandError::InvalidRange("pan must be within -1.0..=1.0"));
            }
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.id == *track_id)
                .ok_or(CommandError::TrackNotFound(*track_id))?;
            track.pan = *pan;
            Ok(None)
        }
        Command::SetTrackMute { track_id, muted } => {
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.id == *track_id)
                .ok_or(CommandError::TrackNotFound(*track_id))?;
            track.muted = *muted;
            Ok(None)
        }
        Command::SetTrackSolo { track_id, solo } => {
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.id == *track_id)
                .ok_or(CommandError::TrackNotFound(*track_id))?;
            track.solo = *solo;
            Ok(None)
        }
        Command::SetClipCutout { clip_id, enabled } => {
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            clip.cutout = *enabled;
            Ok(None)
        }
        Command::SetClipPitch { clip_id, semitones } => {
            if !semitones.is_finite() || *semitones < -24.0 || *semitones > 24.0 {
                return Err(CommandError::InvalidRange(
                    "semitones must be within -24.0..=24.0",
                ));
            }
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            clip.pitch_semitones = *semitones;
            Ok(None)
        }
        Command::SetClipFade {
            clip_id,
            fade_in_ms,
            fade_out_ms,
        } => {
            if *fade_in_ms < 0 || *fade_out_ms < 0 {
                return Err(CommandError::InvalidRange("fade must be >= 0"));
            }
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            if *fade_in_ms > clip.duration_ms || *fade_out_ms > clip.duration_ms {
                return Err(CommandError::InvalidRange(
                    "fade must not exceed the clip duration",
                ));
            }
            clip.fade_in_ms = *fade_in_ms;
            clip.fade_out_ms = *fade_out_ms;
            Ok(None)
        }
        Command::SetTrackColor { track_id, color } => {
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.id == *track_id)
                .ok_or(CommandError::TrackNotFound(*track_id))?;
            track.color = *color;
            Ok(None)
        }
        Command::SetClipColor { clip_id, color } => {
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            clip.color = *color;
            Ok(None)
        }
        Command::SetTrackInstrument {
            track_id,
            instrument,
        } => {
            if let Some(instrument) = instrument {
                validate_instrument(instrument)?;
            }
            let track = project
                .track_mut(*track_id)
                .ok_or(CommandError::TrackNotFound(*track_id))?;
            track.instrument = instrument.clone();
            // 直接の楽器設定が音色参照より優先される（ADR-0018）。
            track.tone_id = None;
            Ok(None)
        }
        Command::AddTone { name, instrument } => {
            validate_tone_name(name)?;
            validate_instrument(instrument)?;
            let id = project.alloc_id();
            project.tones.push(crate::model::Tone {
                id,
                name: name.clone(),
                instrument: instrument.clone(),
            });
            Ok(Some(id))
        }
        Command::RenameTone { tone_id, name } => {
            validate_tone_name(name)?;
            let tone = project
                .tone_mut(*tone_id)
                .ok_or(CommandError::ToneNotFound(*tone_id))?;
            tone.name = name.clone();
            Ok(None)
        }
        Command::UpdateTone {
            tone_id,
            instrument,
        } => {
            validate_instrument(instrument)?;
            let tone = project
                .tone_mut(*tone_id)
                .ok_or(CommandError::ToneNotFound(*tone_id))?;
            tone.instrument = instrument.clone();
            Ok(None)
        }
        Command::RemoveTone { tone_id } => {
            let pos = project
                .tones
                .iter()
                .position(|t| t.id == *tone_id)
                .ok_or(CommandError::ToneNotFound(*tone_id))?;
            project.tones.remove(pos);
            // 参照していたトラックは直接の楽器設定に戻る。
            for track in &mut project.tracks {
                if track.tone_id == Some(*tone_id) {
                    track.tone_id = None;
                }
            }
            Ok(None)
        }
        Command::SetTrackTone { track_id, tone_id } => {
            if let Some(tone_id) = tone_id {
                if project.tone(*tone_id).is_none() {
                    return Err(CommandError::ToneNotFound(*tone_id));
                }
            }
            let track = project
                .track_mut(*track_id)
                .ok_or(CommandError::TrackNotFound(*track_id))?;
            track.tone_id = *tone_id;
            Ok(None)
        }
        Command::AddNote {
            clip_id,
            pitch,
            start_ms,
            duration_ms,
            velocity,
        } => {
            // ノートの不変条件（ADR-0016）: 音高とベロシティは
            // MIDI の範囲、ノートはクリップの範囲内に収まる。
            if *pitch > 127 {
                return Err(CommandError::InvalidRange("pitch must be within 0..=127"));
            }
            if *velocity > 127 {
                return Err(CommandError::InvalidRange(
                    "velocity must be within 0..=127",
                ));
            }
            if *start_ms < 0 {
                return Err(CommandError::InvalidRange("note start_ms must be >= 0"));
            }
            if *duration_ms <= 0 {
                return Err(CommandError::InvalidRange("note duration_ms must be > 0"));
            }
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            // 加算ではなく減算で比べる（start_ms が巨大な入力でも
            // オーバーフローで検証を素通り／panic しないため）。
            if *duration_ms > clip.duration_ms - *start_ms {
                return Err(CommandError::InvalidRange("note must fit within the clip"));
            }
            clip.notes.push(Note {
                pitch: *pitch,
                start_ms: *start_ms,
                duration_ms: *duration_ms,
                velocity: *velocity,
            });
            Ok(None)
        }
        Command::MoveNote {
            clip_id,
            note_index,
            new_pitch,
            new_start_ms,
        } => {
            // 移動後もノートの不変条件（ADR-0016）を保つ。長さは
            // 既存のノートのものをそのまま使う。
            if *new_pitch > 127 {
                return Err(CommandError::InvalidRange("pitch must be within 0..=127"));
            }
            if *new_start_ms < 0 {
                return Err(CommandError::InvalidRange("note start_ms must be >= 0"));
            }
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            let note = clip
                .notes
                .get_mut(*note_index)
                .ok_or(CommandError::InvalidRange("note index out of range"))?;
            // 減算形で比べる（AddNote と同じくオーバーフロー回避）。
            if note.duration_ms > clip.duration_ms - *new_start_ms {
                return Err(CommandError::InvalidRange("note must fit within the clip"));
            }
            note.pitch = *new_pitch;
            note.start_ms = *new_start_ms;
            Ok(None)
        }
        Command::RemoveNote {
            clip_id,
            note_index,
        } => {
            let track = project
                .tracks
                .iter_mut()
                .find(|t| t.clips.iter().any(|c| c.id == *clip_id))
                .ok_or(CommandError::ClipNotFound(*clip_id))?;
            let clip = track
                .clips
                .iter_mut()
                .find(|c| c.id == *clip_id)
                .expect("checked above");
            if *note_index >= clip.notes.len() {
                return Err(CommandError::InvalidRange("note index out of range"));
            }
            clip.notes.remove(*note_index);
            Ok(None)
        }
        // テスト専用（Issue #61）：トラックを追加（id も消費）してから
        // 失敗する。「変更してから失敗する」コマンドの再現。
        #[cfg(test)]
        Command::MutateThenFail => {
            let id = project.alloc_id();
            project.tracks.push(Track {
                id,
                name: "mutated".into(),
                kind: TrackKind::Audio,
                clips: Vec::new(),
                volume: 1.0,
                pan: 0.0,
                color: None,
                muted: false,
                solo: false,
                instrument: None,
                tone_id: None,
            });
            Err(CommandError::InvalidRange("mutate then fail (test only)"))
        }
    }
}
