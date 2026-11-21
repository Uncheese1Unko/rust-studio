//! タイムラインのレイアウト計算と要素識別子（Issue #14）。
//!
//! GUI（nannou）に依存しない純粋関数として置き、単体テストで固める。
//! 座標は「ウィンドウ左上原点・右方向が時間軸」のピクセルで表し、
//! 描画側（main.rs）が nannou の中心原点座標へ変換する。
//! 状態は studio-core の Project が正であり、ここは読み取り専用
//! （状態変更はコマンド層経由のみ、ADR-0004）。

use studio_core::{Project, TrackKind};

/// トラック名を表示する左端ヘッダーの幅（px）。
pub const HEADER_WIDTH_PX: f32 = 120.0;
/// 上端ルーラーの高さ（px）。
pub const RULER_HEIGHT_PX: f32 = 24.0;
/// 1トラック行の高さ（px）。
pub const TRACK_HEIGHT_PX: f32 = 48.0;
/// 画面に表示する時間幅（ms）。M1 では固定幅とする。
pub const VIEW_DURATION_MS: i64 = 10_000;

/// 映像クリップの塗り色（RGB, 0.0–1.0）。
pub const CLIP_VIDEO_COLOR: (f32, f32, f32) = (0.25, 0.45, 0.85);
/// 音声クリップの塗り色（RGB, 0.0–1.0）。
pub const CLIP_AUDIO_COLOR: (f32, f32, f32) = (0.25, 0.70, 0.40);
/// 再生ヘッドの色（RGB, 0.0–1.0）。
pub const PLAYHEAD_COLOR: (f32, f32, f32) = (0.90, 0.20, 0.20);

/// トラック種別ごとのクリップ色。
pub fn clip_color(kind: TrackKind) -> (f32, f32, f32) {
    match kind {
        TrackKind::Video => CLIP_VIDEO_COLOR,
        TrackKind::Audio => CLIP_AUDIO_COLOR,
    }
}

/// 1ミリ秒あたりのピクセル数。
pub fn px_per_ms(view_width_px: f32) -> f32 {
    (view_width_px - HEADER_WIDTH_PX) / VIEW_DURATION_MS as f32
}

/// 時刻（ms）を、ウィンドウ左端からの x ピクセルに変換する。
pub fn time_to_x(ms: i64, view_width_px: f32) -> f32 {
    HEADER_WIDTH_PX + ms as f32 * px_per_ms(view_width_px)
}

/// トラック行の上端 y（ウィンドウ上端からのピクセル）。
pub fn track_top_y(track_index: usize) -> f32 {
    RULER_HEIGHT_PX + track_index as f32 * TRACK_HEIGHT_PX
}

/// クリップ1つの描画矩形（左上原点ピクセル）。
#[derive(Debug, Clone, PartialEq)]
pub struct ClipRect {
    pub clip_id: u64,
    pub track_id: u64,
    pub kind: TrackKind,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// プロジェクトの全クリップの描画矩形を、トラック順・クリップ順で返す。
pub fn layout_clips(project: &Project, view_width_px: f32) -> Vec<ClipRect> {
    let mut rects = Vec::new();
    for (index, track) in project.tracks.iter().enumerate() {
        for clip in &track.clips {
            rects.push(ClipRect {
                clip_id: clip.id,
                track_id: track.id,
                kind: track.kind,
                x: time_to_x(clip.start_ms, view_width_px),
                y: track_top_y(index),
                w: clip.duration_ms as f32 * px_per_ms(view_width_px),
                h: TRACK_HEIGHT_PX,
            });
        }
    }
    rects
}

/// 再生ヘッドの x（ウィンドウ左端からのピクセル）。
pub fn playhead_x(playhead_ms: i64, view_width_px: f32) -> f32 {
    time_to_x(playhead_ms, view_width_px)
}

/// プロジェクトの終端（全クリップの end_ms の最大。空なら 0）。
pub fn project_end_ms(project: &Project) -> i64 {
    project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.end_ms()))
        .max()
        .unwrap_or(0)
}

/// 再生ヘッドを dt_ms 進める。終端に達したら先頭へ巻き戻す。
/// 終端が 0（空プロジェクト）のときは 0 のまま。
pub fn advance_playhead(current_ms: i64, dt_ms: i64, end_ms: i64) -> i64 {
    if end_ms <= 0 {
        return 0;
    }
    let next = current_ms + dt_ms;
    if next >= end_ms {
        next % end_ms
    } else {
        next
    }
}

/// タイムライン全体の識別子（ADR-0005: UI 要素には安定した識別子）。
pub fn timeline_id() -> &'static str {
    "timeline"
}

/// ルーラーの識別子。
pub fn ruler_id() -> &'static str {
    "timeline.ruler"
}

/// 再生ヘッドの識別子。
pub fn playhead_id() -> &'static str {
    "timeline.playhead"
}

/// トラック行の識別子。
pub fn track_element_id(track_id: u64) -> String {
    format!("timeline.track.{track_id}")
}

/// クリップの識別子。
pub fn clip_element_id(clip_id: u64) -> String {
    format!("timeline.clip.{clip_id}")
}

/// プロジェクトに現れる全 UI 要素の識別子を、決定的な順序で返す。
pub fn element_ids(project: &Project) -> Vec<String> {
    let mut ids = vec![
        timeline_id().to_string(),
        ruler_id().to_string(),
        playhead_id().to_string(),
    ];
    for track in &project.tracks {
        ids.push(track_element_id(track.id));
        for clip in &track.clips {
            ids.push(clip_element_id(clip.id));
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::{apply, Command};

    /// コマンド経由でプロジェクトを組み立てる（状態変更はコマンド層のみ）。
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
        let a = apply(
            &mut project,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
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
        apply(
            &mut project,
            &Command::AddClip {
                track_id: a,
                name: "bgm".into(),
                start_ms: 0,
                duration_ms: 8_000,
                source: "demo/bgm.wav".into(),
            },
        )
        .unwrap();
        project
    }

    #[test]
    fn clip_rect_reflects_start_and_duration() {
        let project = sample_project();
        let rects = layout_clips(&project, 960.0);
        let opening = &rects[0];
        // px_per_ms = (960 - 120) / 10_000 = 0.084
        assert!((opening.x - (HEADER_WIDTH_PX + 1_000.0 * 0.084)).abs() < 0.01);
        assert!((opening.w - 2_000.0 * 0.084).abs() < 0.01);
        assert_eq!(opening.y, RULER_HEIGHT_PX);
        assert_eq!(opening.h, TRACK_HEIGHT_PX);
        assert_eq!(opening.kind, TrackKind::Video);
    }

    #[test]
    fn layout_covers_every_clip_in_project() {
        let project = sample_project();
        let rects = layout_clips(&project, 960.0);
        let project_clip_ids: Vec<u64> = project
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter().map(|c| c.id))
            .collect();
        let rect_ids: Vec<u64> = rects.iter().map(|r| r.clip_id).collect();
        assert_eq!(rect_ids, project_clip_ids);
        // 2行目（音声トラック）のクリップは1行分下にずれる
        assert_eq!(rects[1].y, RULER_HEIGHT_PX + TRACK_HEIGHT_PX);
        assert_eq!(rects[1].kind, TrackKind::Audio);
    }

    #[test]
    fn playhead_x_matches_time() {
        assert_eq!(playhead_x(0, 960.0), HEADER_WIDTH_PX);
        let expected = HEADER_WIDTH_PX + 5_000.0 * 0.084;
        assert!((playhead_x(5_000, 960.0) - expected).abs() < 0.01);
    }

    #[test]
    fn advance_playhead_moves_and_wraps() {
        assert_eq!(advance_playhead(100, 50, 1_000), 150);
        assert_eq!(advance_playhead(900, 200, 1_000), 100);
        assert_eq!(advance_playhead(0, 500, 0), 0);
    }

    #[test]
    fn project_end_is_max_clip_end() {
        let project = sample_project();
        assert_eq!(project_end_ms(&project), 8_000);
        assert_eq!(project_end_ms(&Project::new("empty")), 0);
    }

    #[test]
    fn element_ids_are_unique_and_stable() {
        let project = sample_project();
        let first = element_ids(&project);
        let second = element_ids(&project);
        assert_eq!(first, second);
        let mut sorted = first.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), first.len(), "識別子に重複がある: {first:?}");
        assert!(first.iter().any(|id| id == playhead_id()));
        assert!(first.iter().any(|id| id == ruler_id()));
        // トラック2つ＋クリップ2つ＋固定3要素
        assert_eq!(first.len(), 3 + 2 + 2);
    }
}

/// 波形の概形: サンプル列を `columns` 本の柱に分け、各柱のピーク（絶対値
/// 最大）を返す（Issue #14）。描画側はこの値で中心線から上下に線を引く。
/// `samples` はインターリーブ前提で、先頭チャンネルだけを見る。
pub fn waveform_peaks(samples: &[f32], channels: usize, columns: usize) -> Vec<f32> {
    if samples.is_empty() || channels == 0 || columns == 0 {
        return vec![0.0; columns];
    }
    let frames = samples.len() / channels;
    let mut peaks = vec![0.0f32; columns];
    for (i, peak) in peaks.iter_mut().enumerate() {
        let a = i * frames / columns;
        let b = ((i + 1) * frames / columns).max(a + 1).min(frames);
        let mut m = 0.0f32;
        for f in a..b {
            let v = samples[f * channels].abs();
            if v > m {
                m = v;
            }
        }
        *peak = m;
    }
    peaks
}

#[cfg(test)]
mod waveform_tests {
    use super::waveform_peaks;

    #[test]
    fn peaks_capture_amplitude_and_silence() {
        // 前半は振幅0.5の波、後半は無音
        let mut samples = vec![0.5f32; 100];
        samples.extend(vec![0.0f32; 100]);
        let peaks = waveform_peaks(&samples, 1, 4);
        assert_eq!(peaks.len(), 4);
        assert!((peaks[0] - 0.5).abs() < 1e-6);
        assert!((peaks[1] - 0.5).abs() < 1e-6);
        assert_eq!(peaks[2], 0.0);
        assert_eq!(peaks[3], 0.0);
    }

    #[test]
    fn peaks_of_empty_input_are_zero() {
        assert_eq!(waveform_peaks(&[], 1, 3), vec![0.0, 0.0, 0.0]);
        assert_eq!(waveform_peaks(&[0.9], 1, 0), Vec::<f32>::new());
    }

    #[test]
    fn peaks_read_only_the_first_channel_of_interleaved_input() {
        // Issue #14 レビュー指摘: インターリーブ入力では先頭チャンネル
        // だけを見ることを固定する。右チャンネル（大きな振幅）は
        // 拾わない。
        // フレーム列: (L=0.1, R=0.9) x2, (L=0.3, R=0.9) x2
        let samples = vec![0.1f32, 0.9, 0.1, 0.9, 0.3, 0.9, 0.3, 0.9];
        let peaks = waveform_peaks(&samples, 2, 2);
        assert_eq!(peaks.len(), 2);
        assert!((peaks[0] - 0.1).abs() < 1e-6, "got {peaks:?}");
        assert!((peaks[1] - 0.3).abs() < 1e-6, "got {peaks:?}");
    }
}
