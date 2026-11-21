//! video-editor: AviUtl 風の動画編集ソフト（nannou）。
//!
//! Issue #8: タイムライン UI。studio-core の Project（トラック／クリップ）を
//! そのまま描画し、再生ヘッドが時刻に沿って移動する。
//! 状態の変更は studio-core のコマンド層経由でのみ行う（ADR-0004）。
//! レイアウト計算と要素識別子は `timeline` モジュール（GUI 非依存）に置く。
//!
//! 起動時のデモプロジェクトはコマンド経由で構築し、状態ダンプと
//! 要素識別子の一覧を標準出力に出す（ADR-0005 の観測経路）。

mod timeline;

use nannou::prelude::*;
use studio_core::{apply, Command, Project, TrackKind};

struct Model {
    _window: WindowId,
    project: Project,
    playhead_ms: i64,
}

fn main() {
    nannou::app(model).update(update).run();
}

/// デモプロジェクトをコマンド経由で構築する。
/// 状態変更はコマンド層のみという規則（ADR-0004）を GUI 側でも守る。
fn demo_project() -> Project {
    let mut project = Project::new("untitled video");
    let v = apply(
        &mut project,
        &Command::AddTrack {
            name: "V1".into(),
            kind: TrackKind::Video,
        },
    )
    .expect("demo project: add video track")
    .expect("add track returns id");
    let a = apply(
        &mut project,
        &Command::AddTrack {
            name: "A1".into(),
            kind: TrackKind::Audio,
        },
    )
    .expect("demo project: add audio track")
    .expect("add track returns id");
    for (track_id, name, start_ms, duration_ms, source) in [
        (v, "opening", 0, 2_000, "demo/opening.mp4"),
        (v, "interview", 2_500, 4_000, "demo/interview.mp4"),
        (a, "bgm", 0, 8_000, "demo/bgm.wav"),
    ] {
        apply(
            &mut project,
            &Command::AddClip {
                track_id,
                name: name.into(),
                start_ms,
                duration_ms,
                source: source.into(),
            },
        )
        .expect("demo project: add clip");
    }
    project
}

fn model(app: &App) -> Model {
    let project = demo_project();
    // 観測経路（ADR-0005）: 画面に描かれる状態と同一のダンプを出す。
    println!("video-editor (issue #8: timeline UI)");
    println!("{}", project.dump_json());
    println!("elements: {}", timeline::element_ids(&project).join(", "));
    let window = app
        .new_window()
        .size(960, 540)
        .title("video-editor")
        .view(view)
        .build()
        .unwrap();
    Model {
        _window: window,
        project,
        playhead_ms: 0,
    }
}

fn update(_app: &App, model: &mut Model, update: Update) {
    let dt_ms = update.since_last.as_millis() as i64;
    let end_ms = timeline::project_end_ms(&model.project);
    model.playhead_ms = timeline::advance_playhead(model.playhead_ms, dt_ms, end_ms);
}

/// 左上原点ピクセル（x, y, w, h）を nannou の中心原点の矩形中心に変換する。
fn rect_center(win: Rect, x: f32, y: f32, w: f32, h: f32) -> (f32, f32) {
    (x + w / 2.0 - win.w() / 2.0, win.h() / 2.0 - (y + h / 2.0))
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    let win = app.window_rect();
    draw.background().color(rgb(0.09, 0.09, 0.11));

    // ルーラー（timeline.ruler）: 帯＋1秒ごとの目盛りと秒ラベル。
    let (rx, ry) = rect_center(win, 0.0, 0.0, win.w(), timeline::RULER_HEIGHT_PX);
    draw.rect()
        .x_y(rx, ry)
        .w_h(win.w(), timeline::RULER_HEIGHT_PX)
        .color(rgb(0.13, 0.13, 0.16));
    for sec in 0..=(timeline::VIEW_DURATION_MS / 1_000) {
        let x = timeline::time_to_x(sec * 1_000, win.w());
        let nx = x - win.w() / 2.0;
        draw.line()
            .start(pt2(nx, win.h() / 2.0 - timeline::RULER_HEIGHT_PX))
            .end(pt2(nx, win.h() / 2.0 - timeline::RULER_HEIGHT_PX + 6.0))
            .weight(1.0)
            .color(rgb(0.55, 0.55, 0.60));
        draw.text(&format!("{sec}s"))
            .x_y(nx + 10.0, win.h() / 2.0 - timeline::RULER_HEIGHT_PX / 2.0)
            .font_size(10)
            .color(rgb(0.65, 0.65, 0.70));
    }

    // トラック行（timeline.track.<id>）: ヘッダーにトラック名、行背景は交互色。
    for (index, track) in model.project.tracks.iter().enumerate() {
        let top = timeline::track_top_y(index);
        let (bx, by) = rect_center(win, 0.0, top, win.w(), timeline::TRACK_HEIGHT_PX);
        let shade = if index % 2 == 0 { 0.11 } else { 0.14 };
        draw.rect()
            .x_y(bx, by)
            .w_h(win.w(), timeline::TRACK_HEIGHT_PX)
            .color(rgb(shade, shade, shade + 0.02));
        let (hx, hy) = rect_center(
            win,
            0.0,
            top,
            timeline::HEADER_WIDTH_PX,
            timeline::TRACK_HEIGHT_PX,
        );
        draw.rect()
            .x_y(hx, hy)
            .w_h(timeline::HEADER_WIDTH_PX, timeline::TRACK_HEIGHT_PX)
            .color(rgb(0.17, 0.17, 0.20));
        draw.text(&track.name)
            .x_y(hx, hy)
            .font_size(13)
            .color(rgb(0.85, 0.85, 0.88));
    }

    // クリップ（timeline.clip.<id>）: コアの start/duration をそのまま矩形にする。
    for rect in timeline::layout_clips(&model.project, win.w()) {
        let (r, g, b) = timeline::clip_color(rect.kind);
        let (cx, cy) = rect_center(win, rect.x, rect.y + 4.0, rect.w, rect.h - 8.0);
        draw.rect()
            .x_y(cx, cy)
            .w_h(rect.w, rect.h - 8.0)
            .color(rgb(r, g, b));
        if let Some((_, clip)) = model.project.clip(rect.clip_id) {
            draw.text(&clip.name)
                .x_y(cx, cy)
                .font_size(11)
                .color(rgb(0.05, 0.05, 0.08));
        }
    }

    // 再生ヘッド（timeline.playhead）: ルーラー上端から画面下端までの縦線。
    let (pr, pg, pb) = timeline::PLAYHEAD_COLOR;
    let px = timeline::playhead_x(model.playhead_ms, win.w()) - win.w() / 2.0;
    draw.line()
        .start(pt2(px, win.h() / 2.0))
        .end(pt2(px, -win.h() / 2.0))
        .weight(2.0)
        .color(rgb(pr, pg, pb));

    draw.to_frame(app, &frame).unwrap();
}
