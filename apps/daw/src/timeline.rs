//! タイムラインのレイアウト計算と要素識別子（Issue #14）。
//!
//! GUI（nannou）に依存しない純粋関数として置き、単体テストで固める。
//! 座標は「ウィンドウ左上原点・右方向が時間軸」のピクセルで表し、
//! 描画側（main.rs）が nannou の中心原点座標へ変換する。
//! 状態は studio-core の Project が正であり、ここは読み取り専用
//! （状態変更はコマンド層経由のみ、ADR-0004）。

use studio_core::{Clip, Project, TrackKind};

use crate::theme;

/// クリップの色（面, 縁）をテーマから返す（Issue #24、デザイン文書
/// 「現状との対応」）。音声=ソフトピンク面＋ピンクの縁、映像=ミント面
/// ＋ティールの縁。再生中は面がホットピンクになる（強調原則：
/// アクセントは「今」のものだけ）。
pub fn clip_colors(kind: TrackKind, playing: bool) -> (theme::Rgb, theme::Rgb) {
    if playing {
        return (theme::ACCENT, theme::ACCENT);
    }
    match kind {
        TrackKind::Audio => (theme::ACCENT_SOFT, theme::ACCENT),
        TrackKind::Video => (theme::MINT, theme::TEAL),
    }
}

/// 再生ヘッド位置がクリップの範囲内にあるか（再生中クリップの判定）。
/// 範囲は [start_ms, end_ms) で、ピッチ／サンプラーの既存判定と揃える。
/// 読み取りのみで、状態は変えない。
pub fn clip_is_playing(clip: &Clip, playhead_ms: i64) -> bool {
    playhead_ms >= clip.start_ms && playhead_ms < clip.end_ms()
}

/// 再生位置の小節番号（1始まり）。プロジェクトのモデルにテンポが
/// ないため、ルーラーの1秒区切りを1小節として数える（Issue #24）。
pub fn bar_at_ms(playhead_ms: i64) -> i64 {
    playhead_ms.max(0) / 1_000 + 1
}

/// トラック名を表示する左端ヘッダーの幅（px）。
pub const HEADER_WIDTH_PX: f32 = 120.0;
/// 上端ルーラーの高さ（px）。
pub const RULER_HEIGHT_PX: f32 = 24.0;
/// 1トラック行の高さ（px）。
pub const TRACK_HEIGHT_PX: f32 = 48.0;
/// 画面に表示する時間幅（ms）。M1 では固定幅とする。
pub const VIEW_DURATION_MS: i64 = 10_000;

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

#[cfg(test)]
mod theme_tests {
    use super::*;
    use crate::theme;
    use studio_core::{apply, Command};

    fn audio_clip() -> studio_core::Clip {
        let mut project = Project::new("theme test");
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
                track_id: a,
                name: "tone".into(),
                start_ms: 1_000,
                duration_ms: 2_000,
                source: "demo/tone.wav".into(),
            },
        )
        .unwrap();
        project.tracks[0].clips[0].clone()
    }

    #[test]
    fn clip_is_playing_within_start_inclusive_end_exclusive() {
        // Issue #24: 再生中クリップ判定。範囲は [start, end) で、
        // ピッチ／サンプラーの既存判定と揃える。
        let clip = audio_clip();
        assert!(!clip_is_playing(&clip, 999), "開始前は再生中でない");
        assert!(clip_is_playing(&clip, 1_000), "開始位置は含む");
        assert!(clip_is_playing(&clip, 2_999));
        assert!(!clip_is_playing(&clip, 3_000), "終了位置は含まない");
        assert!(!clip_is_playing(&clip, 5_000));
    }

    #[test]
    fn clip_colors_follow_design_doc() {
        // Issue #24: 音声=ソフトピンク面＋ピンクの縁、映像=ミント面＋
        // ティールの縁。再生中だけ面がホットピンクになる。
        assert_eq!(
            clip_colors(TrackKind::Audio, false),
            (theme::ACCENT_SOFT, theme::ACCENT)
        );
        assert_eq!(
            clip_colors(TrackKind::Video, false),
            (theme::MINT, theme::TEAL)
        );
        assert_eq!(
            clip_colors(TrackKind::Audio, true).0,
            theme::ACCENT,
            "再生中は面がホットピンク"
        );
        assert_eq!(clip_colors(TrackKind::Video, true).0, theme::ACCENT);
    }

    #[test]
    fn bar_at_ms_counts_one_second_bars_from_one() {
        // Issue #24: 現在の小節チップ用。モデルにテンポがないため、
        // ルーラーの1秒区切りを1小節として数える（1始まり）。
        assert_eq!(bar_at_ms(0), 1);
        assert_eq!(bar_at_ms(999), 1);
        assert_eq!(bar_at_ms(1_000), 2);
        assert_eq!(bar_at_ms(4_500), 5);
        assert_eq!(bar_at_ms(-100), 1, "負の位置は1小節目に丸める");
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
