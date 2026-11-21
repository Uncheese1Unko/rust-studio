//! video-editor: AviUtl 風の動画編集ソフト（nannou）。
//!
//! Issue #8: タイムライン UI。studio-core の Project（トラック／クリップ）を
//! そのまま描画し、再生ヘッドが時刻に沿って移動する。
//! 状態の変更は studio-core のコマンド層経由でのみ行う（ADR-0004）。
//! レイアウト計算と要素識別子は `timeline` モジュール（GUI 非依存）に置く。
//!
//! 起動時のデモプロジェクトはコマンド経由で構築し、状態ダンプと
//! 要素識別子の一覧を標準出力に出す（ADR-0005 の観測経路）。

mod edit;
mod media;
mod timeline;

use nannou::prelude::*;
use studio_core::{apply, Command, Project, TrackKind};

struct Model {
    _window: WindowId,
    project: Project,
    playhead_ms: i64,
    // Issue #9: ポインタ編集の状態。
    drag: Option<edit::Drag>,
    last_drag_cmd: Option<Command>,
    last_rejection: Option<String>,
    prev_left_down: bool,
    clicks: edit::ClickTracker,
    // Issue #10: プレビュー再生の状態。
    frames: media::FrameStore,
    textures: std::collections::HashMap<std::path::PathBuf, wgpu::Texture>,
    /// (source, frame 番号)。表示フレームが変わったときだけ記録する。
    last_preview: Option<(String, usize)>,
}

/// プレビュー領域（左上原点ピクセル）。要素識別子は `preview`。
const PREVIEW_X: f32 = 500.0;
const PREVIEW_Y: f32 = 150.0;
const PREVIEW_W: f32 = 440.0;
const PREVIEW_H: f32 = 248.0;
const PREVIEW_ID: &str = "preview";

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
    println!(
        "elements: {}, {PREVIEW_ID}",
        timeline::element_ids(&project).join(", ")
    );
    // メディア層（Issue #10）: フレーム列を準備し、テクスチャを読み込む。
    let frames = media::prepare_media(&project);
    let window = app
        .new_window()
        .size(960, 540)
        .title("video-editor")
        .view(view)
        .build()
        .unwrap();
    let mut textures = std::collections::HashMap::new();
    for track in &project.tracks {
        for clip in &track.clips {
            let count = frames.frame_count(&clip.source);
            for i in 0..count {
                let at_ms = (i as i64) * 1_000 / media::FRAME_FPS as i64;
                if let Some(path) = frames.frame_path(&clip.source, at_ms) {
                    if textures.contains_key(path) {
                        continue;
                    }
                    match wgpu::Texture::from_path(app, path) {
                        Ok(texture) => {
                            textures.insert(path.to_path_buf(), texture);
                        }
                        Err(e) => println!("warning: cannot load texture {}: {e}", path.display()),
                    }
                }
            }
        }
    }
    println!("preview textures loaded: {}", textures.len());
    Model {
        _window: window,
        project,
        playhead_ms: 0,
        drag: None,
        last_drag_cmd: None,
        last_rejection: None,
        prev_left_down: false,
        clicks: edit::ClickTracker::new(),
        frames,
        textures,
        last_preview: None,
    }
}

fn update(app: &App, model: &mut Model, update: Update) {
    handle_pointer(app, model);

    let dt_ms = update.since_last.as_millis() as i64;
    let end_ms = timeline::project_end_ms(&model.project);
    model.playhead_ms = timeline::advance_playhead(model.playhead_ms, dt_ms, end_ms);

    // プレビューの表示フレームが変わったら観測用に記録する（Issue #10）。
    let current = media::active_video_clip(&model.project, model.playhead_ms).and_then(|clip| {
        let source_time = media::source_time_ms(clip, model.playhead_ms)?;
        let frame = media::frame_index(source_time, model.frames.fps);
        Some((clip.source.clone(), source_time, frame))
    });
    let current_key = current
        .as_ref()
        .map(|(source, _, frame)| (source.clone(), *frame));
    if current_key != model.last_preview {
        if let Some((source, source_time, frame)) = &current {
            println!(
                "preview: source={source} playhead={} source_time={source_time} frame={frame}",
                model.playhead_ms
            );
        }
        model.last_preview = current_key;
    }
}

/// ポインタ操作をポーリングし、コマンド経由で編集する（Issue #9）。
/// 押下でドラッグ開始（またはダブルクリックで配置）、ドラッグ中は候補
/// コマンドが変わるたびに適用し、解放時に状態ダンプを出す。
/// 範囲外の候補はコマンド層が弾き、状態は最後の正常値に留まる。
fn handle_pointer(app: &App, model: &mut Model) {
    let win = app.window_rect();
    // nannou の中心原点座標を、レイアウトの左上原点ピクセルへ変換する。
    let lx = app.mouse.x + win.w() / 2.0;
    let ly = win.h() / 2.0 - app.mouse.y;
    let left_down = app.mouse.buttons.left().is_down();
    let pointer_ms = edit::x_to_ms(lx, win.w());

    if left_down && !model.prev_left_down {
        let target = edit::hit_test(&model.project, win.w(), lx, ly);
        if let Some(drag) = edit::Drag::begin(&model.project, &target, pointer_ms) {
            model.drag = Some(drag);
            model.last_drag_cmd = None;
            model.last_rejection = None;
        } else if let edit::HitTarget::Empty { track_index } = target {
            let now_ms = (app.time * 1_000.0) as u64;
            if model.clicks.register(now_ms, lx, ly) {
                if let Some(cmd) = edit::place_clip_command(&model.project, track_index, pointer_ms)
                {
                    match apply(&mut model.project, &cmd) {
                        Ok(_) => println!("placed: {}", model.project.dump_json()),
                        Err(e) => println!("command rejected: {e}"),
                    }
                }
            }
        }
    }

    if left_down {
        if let Some(drag) = &model.drag {
            let cmd = drag.command_for(pointer_ms);
            if model.last_drag_cmd.as_ref() != Some(&cmd) {
                match apply(&mut model.project, &cmd) {
                    Ok(_) => model.last_drag_cmd = Some(cmd),
                    Err(e) => model.last_rejection = Some(e.to_string()),
                }
            }
        }
    }

    if !left_down && model.prev_left_down && model.drag.take().is_some() {
        println!("after drag: {}", model.project.dump_json());
        if let Some(rejection) = model.last_rejection.take() {
            println!("command rejected during drag: {rejection}");
        }
        model.last_drag_cmd = None;
    }

    model.prev_left_down = left_down;
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

    // プレビュー（preview）: 再生ヘッド位置の映像クリップのフレームを表示する。
    let (pvx, pvy) = rect_center(win, PREVIEW_X, PREVIEW_Y, PREVIEW_W, PREVIEW_H);
    draw.rect()
        .x_y(pvx, pvy)
        .w_h(PREVIEW_W, PREVIEW_H)
        .color(rgb(0.04, 0.04, 0.05));
    match media::active_video_clip(&model.project, model.playhead_ms) {
        Some(clip) => {
            let source_time = media::source_time_ms(clip, model.playhead_ms).unwrap_or(0);
            if let Some(path) = model.frames.frame_path(&clip.source, source_time) {
                if let Some(texture) = model.textures.get(path) {
                    draw.texture(texture)
                        .x_y(pvx, pvy)
                        .w_h(PREVIEW_W, PREVIEW_H);
                }
            }
            draw.text(&format!("{} @ {}ms", clip.name, source_time))
                .x_y(pvx, pvy + PREVIEW_H / 2.0 + 14.0)
                .font_size(11)
                .color(rgb(0.65, 0.65, 0.70));
        }
        None => {
            draw.text("preview: no video clip")
                .x_y(pvx, pvy)
                .font_size(12)
                .color(rgb(0.45, 0.45, 0.50));
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
