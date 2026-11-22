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

/// ユーザー色を考慮したクリップ色（Issue #58、ADR-0013）。
/// 優先順位: 再生中は常に ACCENT（状態色を守る）→ ユーザー色が
/// あればパレット色 → 未設定なら従来の種別色。
pub fn clip_colors_for(
    color: Option<studio_core::ColorTag>,
    kind: TrackKind,
    playing: bool,
) -> (theme::Rgb, theme::Rgb) {
    if playing {
        return (theme::ACCENT, theme::ACCENT);
    }
    match color {
        Some(tag) => theme::palette_colors(tag),
        None => clip_colors(kind, false),
    }
}

/// 再生ヘッド位置がクリップの範囲内にあるか（再生中クリップの判定）。
/// 範囲は [start_ms, end_ms) で、ピッチ／サンプラーの既存判定と揃える。
/// 読み取りのみで、状態は変えない。
pub fn clip_is_playing(clip: &Clip, playhead_ms: i64) -> bool {
    playhead_ms >= clip.start_ms && playhead_ms < clip.end_ms()
}

/// 1拍の長さ（ミリ秒、Issue #35）。1拍 = 60,000 / bpm。
pub fn beat_duration_ms(bpm: f32) -> f64 {
    60_000.0 / bpm as f64
}

/// 1小節の長さ（ミリ秒、Issue #35）。4/4 固定で 4拍。
pub fn bar_duration_ms(bpm: f32) -> f64 {
    beat_duration_ms(bpm) * 4.0
}

/// 小節頭の位置（ミリ秒、Issue #35）。`bar_index` は 0 始まり。
pub fn bar_start_ms(bpm: f32, bar_index: i64) -> i64 {
    (bar_duration_ms(bpm) * bar_index as f64).round() as i64
}

/// 再生位置の小節番号（1始まり、Issue #35）。BPM から計算する。
/// #24 の仮置き（1秒＝1小節）は bpm=240 の場合に一致する。
pub fn bar_at_ms(playhead_ms: i64, bpm: f32) -> i64 {
    (playhead_ms.max(0) as f64 / bar_duration_ms(bpm)).floor() as i64 + 1
}

/// 時刻（ms）に対する拍番号（1始まり、Issue #35）。BPM から計算する。
pub fn beat_at_ms(ms: i64, bpm: f32) -> i64 {
    let within_bar = ms.max(0) as f64 % bar_duration_ms(bpm);
    (within_bar / beat_duration_ms(bpm)).floor() as i64 + 1
}

/// トラック名を表示する左端ヘッダーの幅（px）。
pub const HEADER_WIDTH_PX: f32 = 120.0;
/// 上端ルーラーの高さ（px）。
pub const RULER_HEIGHT_PX: f32 = 24.0;
/// 1トラック行の高さ（px）。
pub const TRACK_HEIGHT_PX: f32 = 48.0;
/// 画面に表示する時間幅（ms、倍率1のとき）。M1 では固定幅とする。
pub const VIEW_DURATION_MS: i64 = 10_000;

/// ズーム倍率の下限・上限（Issue #38）。
pub const ZOOM_MIN: f32 = 0.25;
pub const ZOOM_MAX: f32 = 8.0;

/// 横方向の表示スケール（Issue #38）: 倍率と左端オフセット。
/// アプリ側の表示状態であり、プロジェクトには保存しない
/// （再起動で既定に戻る）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewScale {
    /// 倍率（0.25..=8.0）。1.0 で VIEW_DURATION_MS が画面幅に収まる。
    pub zoom: f32,
    /// 表示左端（ヘッダー右端）の時刻（ms、0 以上）。
    pub offset_ms: i64,
}

impl Default for ViewScale {
    fn default() -> Self {
        ViewScale {
            zoom: 1.0,
            offset_ms: 0,
        }
    }
}

impl ViewScale {
    /// 1ミリ秒あたりのピクセル数（倍率込み）。
    pub fn px_per_ms(&self, view_width_px: f32) -> f32 {
        px_per_ms(view_width_px) * self.zoom
    }

    /// 時刻（ms）を、ウィンドウ左端からの x ピクセルに変換する。
    pub fn time_to_x(&self, ms: i64, view_width_px: f32) -> f32 {
        HEADER_WIDTH_PX + (ms - self.offset_ms) as f32 * self.px_per_ms(view_width_px)
    }

    /// x ピクセルを時刻（ms）に変換する（time_to_x の逆）。
    pub fn x_to_ms(&self, x: f32, view_width_px: f32) -> i64 {
        self.offset_ms + ((x - HEADER_WIDTH_PX) / self.px_per_ms(view_width_px)).round() as i64
    }

    /// 表示右端の時刻（ms）。
    pub fn end_ms(&self, view_width_px: f32) -> i64 {
        self.x_to_ms(view_width_px, view_width_px)
    }

    /// アンカー（マウス位置の時刻と画面 x）を固定したまま倍率を
    /// 変える（Issue #38）。倍率は範囲にクランプされ、オフセットは
    /// 0 未満にならない。
    pub fn zoom_at(
        &self,
        factor: f32,
        anchor_ms: i64,
        anchor_x: f32,
        view_width_px: f32,
    ) -> ViewScale {
        let zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        let next = ViewScale {
            zoom,
            offset_ms: self.offset_ms,
        };
        let offset = anchor_ms
            - ((anchor_x - HEADER_WIDTH_PX) / next.px_per_ms(view_width_px)).round() as i64;
        ViewScale {
            zoom,
            offset_ms: offset.max(0),
        }
    }

    /// 水平スクロール（オフセットは 0 未満にならない）。
    pub fn scroll_by(&self, delta_ms: i64) -> ViewScale {
        ViewScale {
            offset_ms: (self.offset_ms + delta_ms).max(0),
            ..*self
        }
    }
}

/// 1ミリ秒あたりのピクセル数（倍率1の基準値）。
pub fn px_per_ms(view_width_px: f32) -> f32 {
    (view_width_px - HEADER_WIDTH_PX) / VIEW_DURATION_MS as f32
}

/// 時刻（ms）を、ウィンドウ左端からの x ピクセルに変換する
/// （倍率・オフセットは ViewScale が持つ、Issue #38）。
pub fn time_to_x(ms: i64, view_width_px: f32, scale: &ViewScale) -> f32 {
    scale.time_to_x(ms, view_width_px)
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
pub fn layout_clips(project: &Project, view_width_px: f32, scale: &ViewScale) -> Vec<ClipRect> {
    let mut rects = Vec::new();
    for (index, track) in project.tracks.iter().enumerate() {
        for clip in &track.clips {
            rects.push(ClipRect {
                clip_id: clip.id,
                track_id: track.id,
                kind: track.kind,
                x: time_to_x(clip.start_ms, view_width_px, scale),
                y: track_top_y(index),
                w: clip.duration_ms as f32 * scale.px_per_ms(view_width_px),
                h: TRACK_HEIGHT_PX,
            });
        }
    }
    rects
}

/// 再生ヘッドの x（ウィンドウ左端からのピクセル）。
pub fn playhead_x(playhead_ms: i64, view_width_px: f32, scale: &ViewScale) -> f32 {
    time_to_x(playhead_ms, view_width_px, scale)
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
        let rects = layout_clips(&project, 960.0, &ViewScale::default());
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
        let rects = layout_clips(&project, 960.0, &ViewScale::default());
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
        assert_eq!(playhead_x(0, 960.0, &ViewScale::default()), HEADER_WIDTH_PX);
        let expected = HEADER_WIDTH_PX + 5_000.0 * 0.084;
        assert!((playhead_x(5_000, 960.0, &ViewScale::default()) - expected).abs() < 0.01);
    }

    #[test]
    fn view_scale_zoom_keeps_anchor_and_clamps() {
        // Issue #38: 既定スケールは従来レイアウトと一致する。
        let base = ViewScale::default();
        assert_eq!(base.zoom, 1.0);
        assert_eq!(base.offset_ms, 0);
        assert!((base.time_to_x(5_000, 960.0) - (HEADER_WIDTH_PX + 5_000.0 * 0.084)).abs() < 0.01);
        // アンカー（マウス位置の時刻）を固定して2倍に拡大すると、
        // その時刻の画面位置が変わらない。
        let anchor_x = base.time_to_x(3_000, 960.0);
        let zoomed = base.zoom_at(2.0, 3_000, anchor_x, 960.0);
        assert_eq!(zoomed.zoom, 2.0);
        assert!(
            (zoomed.time_to_x(3_000, 960.0) - anchor_x).abs() <= 1.0,
            "anchor must stay: {} vs {anchor_x}",
            zoomed.time_to_x(3_000, 960.0)
        );
        // 倍率は 0.25..=8 にクランプされる
        assert_eq!(
            base.zoom_at(100.0, 0, HEADER_WIDTH_PX, 960.0).zoom,
            ZOOM_MAX
        );
        assert_eq!(
            base.zoom_at(0.001, 0, HEADER_WIDTH_PX, 960.0).zoom,
            ZOOM_MIN
        );
        // スクロールは 0 未満にならない
        assert_eq!(base.scroll_by(-500).offset_ms, 0);
        assert_eq!(base.scroll_by(700).offset_ms, 700);
    }

    #[test]
    fn advance_playhead_moves_and_wraps() {
        assert_eq!(advance_playhead(100, 50, 1_000), 150);
        assert_eq!(advance_playhead(900, 200, 1_000), 100);
        assert_eq!(advance_playhead(0, 500, 0), 0);
    }

    #[test]
    fn beat_at_ms_counts_four_beats_per_bar() {
        // Issue #35: 拍は BPM から計算する。bpm=240 では1拍250ms・
        // 1小節1000ms になり、従来（#27 の仮置き）と同じ数え方になる。
        assert_eq!(beat_at_ms(0, 240.0), 1);
        assert_eq!(beat_at_ms(249, 240.0), 1);
        assert_eq!(beat_at_ms(250, 240.0), 2);
        assert_eq!(beat_at_ms(999, 240.0), 4);
        assert_eq!(beat_at_ms(1_000, 240.0), 1);
        assert_eq!(beat_at_ms(4_500, 240.0), 3);
        // bpm=120 では1拍500ms
        assert_eq!(beat_at_ms(499, 120.0), 1);
        assert_eq!(beat_at_ms(500, 120.0), 2);
        assert_eq!(beat_at_ms(1_500, 120.0), 4);
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
        // Issue #35: 小節は BPM から計算する（1始まり）。bpm=240 では
        // 1小節1000ms になり、従来（#24 の仮置き）と同じ数え方になる。
        assert_eq!(bar_at_ms(0, 240.0), 1);
        assert_eq!(bar_at_ms(999, 240.0), 1);
        assert_eq!(bar_at_ms(1_000, 240.0), 2);
        assert_eq!(bar_at_ms(4_500, 240.0), 5);
        assert_eq!(bar_at_ms(-100, 240.0), 1, "負の位置は1小節目に丸める");
        // bpm=120 では1小節2000ms
        assert_eq!(bar_at_ms(1_999, 120.0), 1);
        assert_eq!(bar_at_ms(2_000, 120.0), 2);
    }

    #[test]
    fn bar_start_ms_follows_bpm() {
        // Issue #35: 小節頭の位置は BPM で決まる（4/4 固定）。
        // bpm=120 → 1小節 2000ms。
        assert_eq!(bar_start_ms(120.0, 0), 0);
        assert_eq!(bar_start_ms(120.0, 1), 2_000);
        assert_eq!(bar_start_ms(120.0, 2), 4_000);
        assert_eq!(bar_start_ms(120.0, 5), 10_000);
        // bpm=60 → 1小節 4000ms、bpm=240 → 1小節 1000ms
        assert_eq!(bar_start_ms(60.0, 1), 4_000);
        assert_eq!(bar_start_ms(240.0, 3), 3_000);
        // 拍・小節の長さ
        assert_eq!(beat_duration_ms(120.0), 500.0);
        assert_eq!(bar_duration_ms(120.0), 2_000.0);
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

/// クリップ内の表示位置が素材全体のどの割合に当たるか（Issue #32）。
/// 分割で `offset_ms` が進んだクリップは素材の途中から読むため、
/// 波形表示もオフセット分ずらす。割合は 0.0〜1.0 に丸める。
pub fn waveform_source_frac(
    clip_offset_ms: i64,
    within_clip_ms: f32,
    source_duration_ms: i64,
) -> f32 {
    if source_duration_ms <= 0 {
        return 0.0;
    }
    ((clip_offset_ms as f32 + within_clip_ms) / source_duration_ms as f32).clamp(0.0, 1.0)
}

#[cfg(test)]
mod waveform_tests {
    use super::{waveform_peaks, waveform_source_frac};

    #[test]
    fn source_frac_starts_at_clip_offset() {
        // Issue #32: 分割した右クリップ（offset 1000ms、素材 2500ms）は
        // 素材の 0.4 の位置から波形を読み始める。
        assert!((waveform_source_frac(1000, 0.0, 2500) - 0.4).abs() < 1e-6);
        assert!((waveform_source_frac(1000, 500.0, 2500) - 0.6).abs() < 1e-6);
        // オフセットなしの従来クリップは先頭から。
        assert!((waveform_source_frac(0, 500.0, 2500) - 0.2).abs() < 1e-6);
        // 素材の終わりを超えたら 1.0 に丸める。不正な長さは 0。
        assert_eq!(waveform_source_frac(2000, 1000.0, 2500), 1.0);
        assert_eq!(waveform_source_frac(0, 0.0, 0), 0.0);
    }

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

#[cfg(test)]
mod color_tests {
    use super::*;
    use studio_core::ColorTag;

    #[test]
    fn clip_color_falls_back_to_kind_color_when_unset() {
        // Issue #58: 未設定なら従来の種別色（clip_colors）に一致する。
        assert_eq!(
            clip_colors_for(None, TrackKind::Audio, false),
            clip_colors(TrackKind::Audio, false)
        );
        assert_eq!(
            clip_colors_for(None, TrackKind::Video, false),
            clip_colors(TrackKind::Video, false)
        );
        // 設定があればパレット色になる
        assert_eq!(
            clip_colors_for(Some(ColorTag::Sky), TrackKind::Audio, false),
            theme::palette_colors(ColorTag::Sky)
        );
    }

    #[test]
    fn playing_clip_stays_accent_even_with_user_color() {
        // Issue #58 / ADR-0013: 再生中はユーザー色があっても ACCENT で
        // 上書きする（状態色の意味を保つ）。
        assert_eq!(
            clip_colors_for(Some(ColorTag::Mint), TrackKind::Audio, true),
            (theme::ACCENT, theme::ACCENT)
        );
        assert_eq!(
            clip_colors_for(None, TrackKind::Audio, true),
            (theme::ACCENT, theme::ACCENT)
        );
    }
}
