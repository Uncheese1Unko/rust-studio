//! コマンド層：プロジェクト状態への唯一の変更経路（ADR-0004）。
//!
//! GUI・HTTP API・MCP サーバーはすべて、ここで定義したコマンドを
//! JSON で受け取り [`apply`] で適用する。コマンドは検証してから適用され、
//! 失敗したコマンドは状態を変えない。

use serde::{Deserialize, Serialize};

use crate::model::{Clip, Project, Track, TrackKind};

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
                    gain: 1.0,
                    cutout: false,
                    pitch_semitones: 0.0,
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
    }
}
