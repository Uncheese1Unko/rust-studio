//! コマンド層：プロジェクト状態への唯一の変更経路（ADR-0004）。
//!
//! GUI・HTTP API・MCP サーバーはすべて、ここで定義したコマンドを
//! JSON で受け取り [`apply`] で適用する。コマンドは検証してから適用され、
//! 失敗したコマンドは状態を変えない。

use serde::{Deserialize, Serialize};

use crate::model::{Clip, ColorTag, Project, Track, TrackKind};

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
    SetTrackVolume {
        track_id: u64,
        volume: f32,
    },
    SetTrackPan {
        track_id: u64,
        pan: f32,
    },
    SetClipCutout {
        clip_id: u64,
        enabled: bool,
    },
    SetClipPitch {
        clip_id: u64,
        semitones: f32,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    TrackNotFound(u64),
    ClipNotFound(u64),
    InvalidRange(&'static str),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::TrackNotFound(id) => write!(f, "track {id} not found"),
            CommandError::ClipNotFound(id) => write!(f, "clip {id} not found"),
            CommandError::InvalidRange(msg) => write!(f, "invalid range: {msg}"),
        }
    }
}

impl std::error::Error for CommandError {}

/// コマンドを適用する。成功時は新しく作られた要素の id（該当する場合）を返す。
/// 失敗時は状態を一切変えない。
pub fn apply(project: &mut Project, command: &Command) -> Result<Option<u64>, CommandError> {
    match command {
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
            let right = Clip {
                id: new_id,
                name: clip.name.clone(),
                start_ms: *at_ms,
                duration_ms: clip.end_ms() - *at_ms,
                source: clip.source.clone(),
                offset_ms: clip.offset_ms + (*at_ms - clip.start_ms),
                gain: clip.gain,
                cutout: clip.cutout,
                pitch_semitones: clip.pitch_semitones,
                color: clip.color,
            };
            let left_duration = *at_ms - clip.start_ms;
            let track = &mut project.tracks[track_idx];
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
    }
}
