//! ミキサーのレイアウトと値変換（Issue #15）。
//!
//! GUI に依存しない純粋関数として置き、単体テストで固める。
//! ミキサー領域はタイムラインのトラック行の下に、トラックごとの帯として
//! 並ぶ。各帯に音量スライダー（0.0〜2.0）とパンスライダー（-1.0〜1.0）が
//! あり、操作はコアのコマンド（SetTrackVolume / SetTrackPan）になる。

use studio_core::{Command, Project};

use crate::timeline;

/// ミキサー帯1本の高さ（px）。
pub const STRIP_HEIGHT_PX: f32 = 36.0;
/// スライダーの幅（px）。
pub const SLIDER_WIDTH_PX: f32 = 260.0;
/// 音量スライダーの左端（px、ヘッダーの右）。
pub const VOLUME_X: f32 = timeline::HEADER_WIDTH_PX + 60.0;
/// パンスライダーの左端（px）。
pub const PAN_X: f32 = VOLUME_X + SLIDER_WIDTH_PX + 80.0;

/// ミキサー領域の上端 y（トラック行の直下）。
pub fn mixer_top_y(track_count: usize) -> f32 {
    timeline::RULER_HEIGHT_PX + track_count as f32 * timeline::TRACK_HEIGHT_PX
}

/// 矩形（左上原点ピクセル）。
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

fn strip_y(track_count: usize, track_index: usize) -> f32 {
    mixer_top_y(track_count) + track_index as f32 * STRIP_HEIGHT_PX
}

/// 音量スライダーの矩形。
pub fn volume_rect(track_count: usize, track_index: usize) -> Rect {
    Rect {
        x: VOLUME_X,
        y: strip_y(track_count, track_index) + 8.0,
        w: SLIDER_WIDTH_PX,
        h: 20.0,
    }
}

/// パンスライダーの矩形。
pub fn pan_rect(track_count: usize, track_index: usize) -> Rect {
    Rect {
        x: PAN_X,
        y: strip_y(track_count, track_index) + 8.0,
        w: SLIDER_WIDTH_PX,
        h: 20.0,
    }
}

/// x から音量（0.0〜2.0、範囲外はクランプ）。
pub fn volume_at(rect: Rect, x: f32) -> f32 {
    let frac = ((x - rect.x) / rect.w).clamp(0.0, 1.0);
    frac * 2.0
}

/// 音量からつまみの x。
pub fn x_for_volume(rect: Rect, volume: f32) -> f32 {
    rect.x + (volume.clamp(0.0, 2.0) / 2.0) * rect.w
}

/// x からパン（-1.0〜1.0、範囲外はクランプ）。
pub fn pan_at(rect: Rect, x: f32) -> f32 {
    let frac = ((x - rect.x) / rect.w).clamp(0.0, 1.0);
    frac * 2.0 - 1.0
}

/// パンからつまみの x。
pub fn x_for_pan(rect: Rect, pan: f32) -> f32 {
    rect.x + ((pan.clamp(-1.0, 1.0) + 1.0) / 2.0) * rect.w
}

/// ミキサー操作の対象パラメータ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Param {
    Volume,
    Pan,
}

/// ヒット結果: どのトラックのどのパラメータを、いくつの値にするか。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixerHit {
    pub track_id: u64,
    pub param: Param,
    pub value: f32,
}

/// 座標に対するミキサーのヒットテスト。
pub fn hit_mixer(project: &Project, x: f32, y: f32) -> Option<MixerHit> {
    let track_count = project.tracks.len();
    for (index, track) in project.tracks.iter().enumerate() {
        let v = volume_rect(track_count, index);
        if v.contains(x, y) {
            return Some(MixerHit {
                track_id: track.id,
                param: Param::Volume,
                value: volume_at(v, x),
            });
        }
        let p = pan_rect(track_count, index);
        if p.contains(x, y) {
            return Some(MixerHit {
                track_id: track.id,
                param: Param::Pan,
                value: pan_at(p, x),
            });
        }
    }
    None
}

/// ヒット結果からコマンドを作る。
pub fn command_for(hit: &MixerHit) -> Command {
    match hit.param {
        Param::Volume => Command::SetTrackVolume {
            track_id: hit.track_id,
            volume: hit.value,
        },
        Param::Pan => Command::SetTrackPan {
            track_id: hit.track_id,
            pan: hit.value,
        },
    }
}

/// 同じトラック・同じパラメータを指したまま、x だけ変えたヒットを作る
/// （ドラッグ継続用）。
pub fn hit_with_x(project: &Project, track_id: u64, param: Param, x: f32) -> Option<MixerHit> {
    let track_count = project.tracks.len();
    let index = project.tracks.iter().position(|t| t.id == track_id)?;
    let (rect, value) = match param {
        Param::Volume => {
            let r = volume_rect(track_count, index);
            (r, volume_at(r, x))
        }
        Param::Pan => {
            let r = pan_rect(track_count, index);
            (r, pan_at(r, x))
        }
    };
    let _ = rect;
    Some(MixerHit {
        track_id,
        param,
        value,
    })
}

/// 音量の表示用フォーマット（dB、Issue #25）。1.0 を 0dB とし、
/// 0 以下は無音として `-inf dB` と表示する。
pub fn format_volume_db(volume: f32) -> String {
    if volume <= 0.0 {
        "-inf dB".to_string()
    } else {
        format!("{:+.1} dB", 20.0 * volume.log10())
    }
}

/// パンの表示用フォーマット（Issue #25）。中央は `C`、左右は
/// `L50` / `R100` のように百分率で示す。
pub fn format_pan(pan: f32) -> String {
    let pan = pan.clamp(-1.0, 1.0);
    let percent = (pan.abs() * 100.0).round() as i32;
    if percent == 0 {
        "C".to_string()
    } else if pan < 0.0 {
        format!("L{percent}")
    } else {
        format!("R{percent}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::{apply, TrackKind};

    fn sample_project() -> Project {
        let mut p = Project::new("mixer");
        apply(
            &mut p,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap();
        p
    }

    #[test]
    fn slider_mappings_clamp_and_round_trip() {
        let v = volume_rect(1, 0);
        assert_eq!(volume_at(v, v.x), 0.0);
        assert_eq!(volume_at(v, v.x + v.w), 2.0);
        assert_eq!(volume_at(v, v.x - 50.0), 0.0, "clamped at left");
        assert_eq!(volume_at(v, v.x + v.w + 50.0), 2.0, "clamped at right");
        assert!((x_for_volume(v, 1.0) - (v.x + v.w / 2.0)).abs() < 0.01);
        let p = pan_rect(1, 0);
        assert_eq!(pan_at(p, p.x), -1.0);
        assert_eq!(pan_at(p, p.x + p.w), 1.0);
        assert!((pan_at(p, x_for_pan(p, 0.25)) - 0.25).abs() < 0.01);
    }

    #[test]
    fn value_formats_for_display() {
        // Issue #25: 選択中トラックの現在値を大きく出すための表示
        // フォーマット。音量は dB、パンは L-C-R 表記。
        assert_eq!(format_volume_db(1.0), "+0.0 dB");
        assert_eq!(format_volume_db(0.5), "-6.0 dB");
        assert_eq!(format_volume_db(2.0), "+6.0 dB");
        assert_eq!(format_volume_db(0.25), "-12.0 dB");
        assert_eq!(format_volume_db(0.0), "-inf dB");
        assert_eq!(format_pan(0.0), "C");
        assert_eq!(format_pan(-1.0), "L100");
        assert_eq!(format_pan(1.0), "R100");
        assert_eq!(format_pan(-0.25), "L25");
        assert_eq!(format_pan(0.5), "R50");
    }

    #[test]
    fn hit_mixer_builds_commands() {
        let p = sample_project();
        let track_id = p.tracks[0].id;
        let v = volume_rect(1, 0);
        let hit = hit_mixer(&p, v.x + v.w * 0.25, v.y + 10.0).unwrap();
        assert_eq!(hit.track_id, track_id);
        assert_eq!(hit.param, Param::Volume);
        assert!((hit.value - 0.5).abs() < 0.01);
        assert_eq!(
            command_for(&hit),
            Command::SetTrackVolume {
                track_id,
                volume: hit.value
            }
        );
        let pr = pan_rect(1, 0);
        let hit = hit_mixer(&p, pr.x + pr.w - 0.5, pr.y + 10.0).unwrap();
        assert_eq!(hit.param, Param::Pan);
        assert!(
            hit.value > 0.99,
            "right end should be ~1.0, was {}",
            hit.value
        );
        // ミキサー領域の外は None
        assert!(hit_mixer(&p, v.x + 10.0, 10.0).is_none());
        assert!(hit_mixer(&p, 10.0, v.y + 10.0).is_none());
    }
}
