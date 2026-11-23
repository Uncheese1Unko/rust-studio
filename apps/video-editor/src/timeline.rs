//! タイムラインのレイアウト計算と要素識別子（Issue #8）。
//!
//! GUI（nannou）に依存しない純粋関数として置き、単体テストで固める。
//! 座標は「ウィンドウ左上原点・右方向が時間軸」のピクセルで表し、
//! 描画側（main.rs）が nannou の中心原点座標へ変換する。
//! 状態は studio-core の Project が正であり、ここは読み取り専用
//! （状態変更はコマンド層経由のみ、ADR-0004）。

use studio_core::{Clip, ColorTag, Project, TrackKind};

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

/// ユーザー色のパレット（Issue #59、ADR-0013）。コアの `ColorTag` に
/// 対応する、動画編集の暗い画面に合わせた表示色（RGB, 0.0–1.0）。
/// 色の保持は識別子のみで、RGB はアプリ側の表現としてここに置く。
pub const PALETTE: [(ColorTag, (f32, f32, f32)); 8] = [
    (ColorTag::Pink, (0.95, 0.45, 0.65)),
    (ColorTag::Coral, (0.95, 0.50, 0.35)),
    (ColorTag::Peach, (0.95, 0.70, 0.40)),
    (ColorTag::Lemon, (0.90, 0.80, 0.30)),
    (ColorTag::Mint, (0.40, 0.90, 0.75)),
    (ColorTag::Teal, (0.20, 0.75, 0.70)),
    (ColorTag::Sky, (0.35, 0.65, 0.95)),
    (ColorTag::Lavender, (0.65, 0.55, 0.95)),
];

/// パレット識別子から表示色を引く。
pub fn palette_color(tag: ColorTag) -> (f32, f32, f32) {
    PALETTE
        .iter()
        .find(|(t, _)| *t == tag)
        .map(|(_, c)| *c)
        .expect("PALETTE は全タグを持つ")
}

/// クリップの表示色（Issue #59）: ユーザー色があればそれを、
/// 未設定なら従来の種別色を使う。
pub fn clip_display_color(color: Option<ColorTag>, kind: TrackKind) -> (f32, f32, f32) {
    match color {
        Some(tag) => palette_color(tag),
        None => clip_color(kind),
    }
}

/// トラックの表示色（Issue #59、ヘッダーの色帯用）: クリップと同じ
/// 規則（ユーザー色優先、未設定は種別色）。
pub fn track_display_color(color: Option<ColorTag>, kind: TrackKind) -> (f32, f32, f32) {
    clip_display_color(color, kind)
}

/// 1ミリ秒あたりのピクセル数（倍率1の基準値）。
pub fn px_per_ms(view_width_px: f32) -> f32 {
    (view_width_px - HEADER_WIDTH_PX) / VIEW_DURATION_MS as f32
}

/// ズーム倍率の範囲（Issue #46）。
pub const ZOOM_MIN: f32 = 0.25;
pub const ZOOM_MAX: f32 = 8.0;

/// タイムラインの表示スケール（Issue #46）。倍率と左端オフセットは
/// アプリ側の表示状態であり、プロジェクトには保存しない。
/// DAW（#38）の ViewScale と同一設計。
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
    /// 変える（Issue #46）。倍率は範囲にクランプされ、オフセットは
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

    /// 表示中の時間幅（ms）＝表示右端 − 左端オフセット。
    pub fn visible_ms(&self, view_width_px: f32) -> i64 {
        self.end_ms(view_width_px) - self.offset_ms
    }

    /// 再生ヘッドへの表示追従（Issue #84）。ページ送り方式：
    /// 再生ヘッドが表示右端を超えた場合にだけ、表示幅の整数倍だけ
    /// オフセットを進めて再生ヘッドを表示範囲の左寄りに戻す。
    /// 範囲内・表示範囲より前（左）にある場合は動かさない（手動
    /// スクロール直後に追従が表示を奪い返さないため）。
    /// `wrapped`（終端で先頭へ巻き戻った）のときはオフセットを
    /// 0 に戻す。
    pub fn follow_playhead(
        &self,
        playhead_ms: i64,
        view_width_px: f32,
        wrapped: bool,
    ) -> ViewScale {
        if wrapped {
            return ViewScale {
                offset_ms: 0,
                ..*self
            };
        }
        let end = self.end_ms(view_width_px);
        if playhead_ms <= end {
            return *self;
        }
        let visible = (end - self.offset_ms).max(1);
        let pages = (playhead_ms - end + visible - 1) / visible;
        ViewScale {
            offset_ms: self.offset_ms + pages * visible,
            ..*self
        }
    }
}

/// フェードの傾斜（Issue #51）。各端点は (x, y) ピクセル。
/// フェードインは左下→上へ、フェードアウトは上→右下への対角線。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FadeSlopes {
    pub fade_in: Option<((f32, f32), (f32, f32))>,
    pub fade_out: Option<((f32, f32), (f32, f32))>,
}

/// クリップ矩形に対するフェード傾斜の端点（Issue #51）。
/// 両フェードとも 0 のときは None。幅はクリップ矩形でクランプする。
pub fn fade_slopes(
    rect: &ClipRect,
    clip: &Clip,
    view_width_px: f32,
    scale: &ViewScale,
) -> Option<FadeSlopes> {
    if clip.fade_in_ms <= 0 && clip.fade_out_ms <= 0 {
        return None;
    }
    let ppm = scale.px_per_ms(view_width_px);
    let fade_in = (clip.fade_in_ms > 0).then(|| {
        let w = (clip.fade_in_ms as f32 * ppm).min(rect.w);
        ((rect.x, rect.y + rect.h), (rect.x + w, rect.y))
    });
    let fade_out = (clip.fade_out_ms > 0).then(|| {
        let w = (clip.fade_out_ms as f32 * ppm).min(rect.w);
        (
            (rect.x + rect.w - w, rect.y),
            (rect.x + rect.w, rect.y + rect.h),
        )
    });
    Some(FadeSlopes { fade_in, fade_out })
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
    /// 左端がヘッダー境界で切り取られたか（元の左端は画面外）。
    /// ヒットテストが実在しない左端を掴み位置にしないために使う
    /// （Issue #46 レビュー round 2 指摘）。
    pub clipped_left: bool,
}

/// プロジェクトの全クリップの描画矩形を、トラック順・クリップ順で返す。
/// 矩形はヘッダー列の右端で切り取る（Issue #46 レビュー指摘）:
/// スクロールで左端が画面外に出たクリップがヘッダー（トラック名や
/// 削除ボタン）の上へ描かれないようにし、ヒットテストの境界
/// （x < HEADER_WIDTH_PX は Outside）と一致させる。
pub fn layout_clips(project: &Project, view_width_px: f32, scale: &ViewScale) -> Vec<ClipRect> {
    let mut rects = Vec::new();
    for (index, track) in project.tracks.iter().enumerate() {
        for clip in &track.clips {
            let raw_x = scale.time_to_x(clip.start_ms, view_width_px);
            let raw_w = clip.duration_ms as f32 * scale.px_per_ms(view_width_px);
            let x = raw_x.max(HEADER_WIDTH_PX);
            let w = (raw_x + raw_w - x).max(0.0);
            rects.push(ClipRect {
                clip_id: clip.id,
                track_id: track.id,
                kind: track.kind,
                x,
                y: track_top_y(index),
                w,
                h: TRACK_HEIGHT_PX,
                clipped_left: raw_x < HEADER_WIDTH_PX,
            });
        }
    }
    rects
}

/// 再生ヘッドの x（ウィンドウ左端からのピクセル）。
pub fn playhead_x(playhead_ms: i64, view_width_px: f32, scale: &ViewScale) -> f32 {
    scale.time_to_x(playhead_ms, view_width_px)
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
    fn layout_clips_rects_at_header_edge_when_scrolled() {
        // Issue #46 レビュー指摘: オフセットより前から始まるクリップは
        // ヘッダー列（x < HEADER_WIDTH_PX）へはみ出さず、見えている
        // 部分だけの矩形になる（描画とヒットテストの境界を一致させる）。
        let project = sample_project();
        let scale = ViewScale {
            zoom: 1.0,
            offset_ms: 1_500,
        };
        let rects = layout_clips(&project, 960.0, &scale);
        let opening = &rects[0]; // 1000..3000、左端は画面外
        assert_eq!(opening.x, HEADER_WIDTH_PX);
        assert!(opening.clipped_left);
        // 見えているのは 1500..3000 の 1500ms 分
        assert!(
            (opening.w - 1_500.0 * 0.084).abs() < 0.01,
            "w={}",
            opening.w
        );
        // 完全に表示範囲より前のクリップは幅 0 になる
        let mut p2 = Project::new("t");
        let v = studio_core::apply(
            &mut p2,
            &studio_core::Command::AddTrack {
                name: "V1".into(),
                kind: TrackKind::Video,
            },
        )
        .unwrap()
        .unwrap();
        studio_core::apply(
            &mut p2,
            &studio_core::Command::AddClip {
                track_id: v,
                name: "past".into(),
                start_ms: 0,
                duration_ms: 500,
                source: "demo/opening.mp4".into(),
            },
        )
        .unwrap();
        let rects2 = layout_clips(&p2, 960.0, &scale);
        assert_eq!(rects2[0].x, HEADER_WIDTH_PX);
        assert_eq!(rects2[0].w, 0.0);
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
        // Issue #46: ズームはマウス位置の時刻（アンカー）の画面位置を
        // 保ち、倍率は 0.25..=8、オフセットは 0 以上にクランプされる。
        let base = ViewScale::default();
        assert_eq!(base.zoom, 1.0);
        assert_eq!(base.offset_ms, 0);
        let anchor_x = base.time_to_x(3_000, 960.0);
        let zoomed = base.zoom_at(2.0, 3_000, anchor_x, 960.0);
        assert_eq!(zoomed.zoom, 2.0);
        assert!((zoomed.time_to_x(3_000, 960.0) - anchor_x).abs() < 0.01);
        assert_eq!(zoomed.offset_ms, 1_500);
        // 倍率のクランプ
        assert_eq!(base.zoom_at(100.0, 0, HEADER_WIDTH_PX, 960.0).zoom, 8.0);
        assert_eq!(base.zoom_at(0.001, 0, HEADER_WIDTH_PX, 960.0).zoom, 0.25);
        // スクロールのクランプ
        assert_eq!(base.scroll_by(-500).offset_ms, 0);
        assert_eq!(base.scroll_by(700).offset_ms, 700);
        // 既定スケールでは従来の換算（ヘッダー＋ms×基準 px_per_ms）と一致する
        assert_eq!(
            base.time_to_x(5_000, 960.0),
            HEADER_WIDTH_PX + 5_000.0 * px_per_ms(960.0)
        );
        assert_eq!(base.px_per_ms(960.0), px_per_ms(960.0));
    }

    #[test]
    fn follow_playhead_pages_forward_past_right_edge() {
        // Issue #84: 再生ヘッドが表示右端を超えたら、表示幅1ページ分
        // オフセットが進み、再生ヘッドが表示範囲の左寄りに戻る。
        // 幅 960・倍率1では表示幅は 10,000ms。
        let base = ViewScale::default();
        // 右端ちょうど（10,000）はまだ範囲内なので動かない
        assert_eq!(base.follow_playhead(10_000, 960.0, false).offset_ms, 0);
        // 超えたら1ページ進む
        let followed = base.follow_playhead(10_500, 960.0, false);
        assert_eq!(followed.offset_ms, 10_000);
        assert_eq!(followed.zoom, base.zoom);
        // 再生ヘッドが新しい表示範囲内にある
        assert!(followed.offset_ms <= 10_500 && 10_500 <= followed.end_ms(960.0));
        // 大きく先にある場合は複数ページ進む
        assert_eq!(base.follow_playhead(25_000, 960.0, false).offset_ms, 20_000);
        // ズーム2倍では表示幅 5,000ms なのでページも 5,000ms
        let zoomed = ViewScale {
            zoom: 2.0,
            offset_ms: 0,
        };
        assert_eq!(zoomed.follow_playhead(5_500, 960.0, false).offset_ms, 5_000);
    }

    #[test]
    fn follow_playhead_keeps_offset_when_playhead_in_view_or_before() {
        // Issue #84: 範囲内では動かさない。表示範囲より前（左）に
        // ある場合も、手動スクロール直後に表示を奪い返さないため
        // 動かさない。
        let base = ViewScale {
            zoom: 1.0,
            offset_ms: 3_000,
        };
        assert_eq!(base.follow_playhead(5_000, 960.0, false).offset_ms, 3_000);
        assert_eq!(base.follow_playhead(13_000, 960.0, false).offset_ms, 3_000);
        assert_eq!(base.follow_playhead(2_000, 960.0, false).offset_ms, 3_000);
        assert_eq!(base.follow_playhead(0, 960.0, false).offset_ms, 3_000);
    }

    #[test]
    fn follow_playhead_resets_offset_on_wrap() {
        // Issue #84: ラップアラウンド（終端で先頭へ巻き戻り）時は
        // オフセットも 0 に戻る。
        let base = ViewScale {
            zoom: 1.0,
            offset_ms: 20_000,
        };
        let followed = base.follow_playhead(120, 960.0, true);
        assert_eq!(followed.offset_ms, 0);
        assert_eq!(followed.zoom, base.zoom);
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

    #[test]
    fn clip_display_color_prefers_user_color_over_kind_color() {
        // Issue #59: ユーザー色があれば種別色より優先する。
        assert_eq!(
            clip_display_color(Some(ColorTag::Coral), TrackKind::Video),
            palette_color(ColorTag::Coral)
        );
        assert_ne!(
            clip_display_color(Some(ColorTag::Coral), TrackKind::Video),
            clip_color(TrackKind::Video)
        );
        // 未設定なら従来の種別色。
        assert_eq!(
            clip_display_color(None, TrackKind::Video),
            clip_color(TrackKind::Video)
        );
        assert_eq!(
            clip_display_color(None, TrackKind::Audio),
            clip_color(TrackKind::Audio)
        );
    }

    #[test]
    fn track_display_color_falls_back_when_unset() {
        // Issue #59: トラック色が未設定なら種別色にフォールバックし、
        // 設定があればパレット色を返す（ヘッダーの色帯用）。
        assert_eq!(
            track_display_color(None, TrackKind::Video),
            clip_color(TrackKind::Video)
        );
        assert_eq!(
            track_display_color(None, TrackKind::Audio),
            clip_color(TrackKind::Audio)
        );
        assert_eq!(
            track_display_color(Some(ColorTag::Sky), TrackKind::Video),
            palette_color(ColorTag::Sky)
        );
    }

    #[test]
    fn palette_covers_all_tags() {
        // Issue #59: パレットは ColorTag の全バリアントを持つ。
        for tag in [
            ColorTag::Pink,
            ColorTag::Coral,
            ColorTag::Peach,
            ColorTag::Lemon,
            ColorTag::Mint,
            ColorTag::Teal,
            ColorTag::Sky,
            ColorTag::Lavender,
        ] {
            let _ = palette_color(tag);
        }
    }

    #[test]
    fn fade_slopes_follow_fade_lengths() {
        // Issue #51: フェードの傾斜は、フェードインが左下→上、
        // フェードアウトが上→右下になる対角線。長さ 0 の側は出ない。
        let mut p = Project::new("t");
        let v = apply(
            &mut p,
            &Command::AddTrack {
                name: "V1".into(),
                kind: TrackKind::Video,
            },
        )
        .unwrap()
        .unwrap();
        let clip_id = apply(
            &mut p,
            &Command::AddClip {
                track_id: v,
                name: "c".into(),
                start_ms: 1_000,
                duration_ms: 2_000,
                source: "demo/opening.mp4".into(),
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::SetClipFade {
                clip_id,
                fade_in_ms: 500,
                fade_out_ms: 250,
            },
        )
        .unwrap();
        let scale = ViewScale::default();
        let rects = layout_clips(&p, 960.0, &scale);
        let rect = &rects[0];
        let (_, clip) = p.clip(clip_id).unwrap();
        let slopes = fade_slopes(rect, clip, 960.0, &scale).expect("slopes exist");
        // フェードイン: 左下 (x, y+h) → (x + 500ms 幅, y)
        let fin = slopes.fade_in.expect("fade-in slope");
        assert_eq!(fin.0, (rect.x, rect.y + rect.h));
        assert!((fin.1 .0 - (rect.x + 500.0 * scale.px_per_ms(960.0))).abs() < 0.01);
        assert_eq!(fin.1 .1, rect.y);
        // フェードアウト: (右端 - 250ms 幅, y) → 右下 (x+w, y+h)
        let fout = slopes.fade_out.expect("fade-out slope");
        assert!((fout.0 .0 - (rect.x + rect.w - 250.0 * scale.px_per_ms(960.0))).abs() < 0.01);
        assert_eq!(fout.0 .1, rect.y);
        assert_eq!(fout.1, (rect.x + rect.w, rect.y + rect.h));
        // フェードが両方 0 のクリップには傾斜が出ない
        apply(
            &mut p,
            &Command::SetClipFade {
                clip_id,
                fade_in_ms: 0,
                fade_out_ms: 0,
            },
        )
        .unwrap();
        let (_, clip) = p.clip(clip_id).unwrap();
        assert!(fade_slopes(rect, clip, 960.0, &scale).is_none());
    }
}
