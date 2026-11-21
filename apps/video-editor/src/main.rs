//! video-editor: AviUtl 風の動画編集ソフト（nannou）。
//!
//! Issue #8: タイムライン UI。studio-core の Project（トラック／クリップ）を
//! そのまま描画し、再生ヘッドが時刻に沿って移動する。
//! 状態の変更は studio-core のコマンド層経由でのみ行う（ADR-0004）。
//! レイアウト計算と要素識別子は `timeline` モジュール（GUI 非依存）に置く。
//!
//! 起動時のデモプロジェクトはコマンド経由で構築し、状態ダンプと
//! 要素識別子の一覧を標準出力に出す（ADR-0005 の観測経路）。

mod cutout;
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
    // Issue #11: プロジェクトファイル共有モードの状態。
    project_file: Option<std::path::PathBuf>,
    file_stamp: u64,
    last_watch_check: f32,
    // Issue #22: 人物切り抜きジョブの状態。
    cutout_job: Option<std::sync::Arc<std::sync::Mutex<cutout::JobState>>>,
    cutout_job_reported: bool,
    prev_k_down: bool,
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
    // 背景用の映像トラック（Issue #22 の合成先）
    let v2 = apply(
        &mut project,
        &Command::AddTrack {
            name: "V2".into(),
            kind: TrackKind::Video,
        },
    )
    .expect("demo project: add background track")
    .expect("add track returns id");
    for (track_id, name, start_ms, duration_ms, source) in [
        (v, "opening", 0, 2_000, "demo/opening.mp4"),
        (v, "interview", 2_500, 4_000, "demo/interview.mp4"),
        (a, "bgm", 0, 8_000, "demo/bgm.wav"),
        (v2, "bg", 0, 8_000, "demo/bg.mp4"),
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

/// プロジェクトファイル共有モード（Issue #11、ADR-0010）の入口。
/// 環境変数 `STUDIO_PROJECT_FILE` があればそのファイルのプロジェクトを、
/// なければデモプロジェクトを返す。ファイルの内容スタンプも返す
/// （変更検出の初期値。読めなければデモにフォールバックする）。
fn initial_project() -> (Project, Option<std::path::PathBuf>, u64) {
    let path = std::env::var_os("STUDIO_PROJECT_FILE").map(std::path::PathBuf::from);
    if let Some(path) = path {
        match Project::load_from_path(&path) {
            Ok(project) => {
                let stamp = std::fs::read(&path)
                    .map(|bytes| media::fnv1a(&bytes))
                    .unwrap_or(0);
                println!("project file mode: {}", path.display());
                return (project, Some(path), stamp);
            }
            Err(e) => {
                println!(
                    "warning: cannot load project file {}: {e}; using demo project",
                    path.display()
                );
            }
        }
    }
    (demo_project(), None, 0)
}

/// フレーム列のテクスチャをすべて読み込む。
fn load_textures(
    app: &App,
    project: &Project,
    frames: &media::FrameStore,
) -> std::collections::HashMap<std::path::PathBuf, wgpu::Texture> {
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
    textures
}

fn model(app: &App) -> Model {
    let (project, project_file, file_stamp) = initial_project();
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
    let textures = load_textures(app, &project, &frames);
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
        project_file,
        file_stamp,
        last_watch_check: 0.0,
        cutout_job: None,
        cutout_job_reported: false,
        prev_k_down: false,
    }
}

/// プロジェクトファイルの変更を監視し、変わっていたら読み直す
/// （Issue #11）。MCP 側の編集を GUI に反映する経路。読み込みに失敗
/// （書き込み途中の読み等）した場合は現在の状態を保ち、次回に再試行する。
fn watch_project_file(app: &App, model: &mut Model) {
    let Some(path) = model.project_file.clone() else {
        return;
    };
    if app.time - model.last_watch_check < 0.5 {
        return;
    }
    model.last_watch_check = app.time;
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let stamp = media::fnv1a(&bytes);
    if stamp == model.file_stamp {
        return;
    }
    match Project::load_from_path(&path) {
        Ok(new_project) => {
            model.file_stamp = stamp;
            // 外部変更で状態が差し替わるため、旧状態前提のドラッグは
            // 打ち切る（Issue #11 レビュー対応）。
            model.drag = None;
            model.last_drag_cmd = None;
            model.last_rejection = None;
            // ソース集合が変わったらメディアも準備し直す
            let mut new_sources: Vec<String> = new_project
                .tracks
                .iter()
                .flat_map(|t| t.clips.iter().map(|c| c.source.clone()))
                .collect();
            new_sources.sort();
            new_sources.dedup();
            let mut old_sources = model.frames.sources();
            old_sources.sort();
            model.project = new_project;
            if new_sources != old_sources {
                model.frames = media::prepare_media(&model.project);
                model.textures = load_textures(app, &model.project, &model.frames);
            }
            println!("project reloaded: {}", model.project.dump_json());
        }
        Err(e) => {
            println!("warning: project reload failed: {e}");
        }
    }
}

/// GUI の編集結果をプロジェクトファイルへ保存し直す（Issue #11）。
/// 自分の保存で監視が再読込しないよう、スタンプも同期させる。
fn save_project_file(model: &mut Model) {
    let Some(path) = model.project_file.clone() else {
        return;
    };
    match model.project.save_to_path(&path) {
        Ok(()) => {
            model.file_stamp = media::fnv1a(model.project.dump_json().as_bytes());
        }
        Err(e) => println!("warning: cannot save project file {}: {e}", path.display()),
    }
}

fn update(app: &App, model: &mut Model, update: Update) {
    handle_pointer(app, model);
    watch_project_file(app, model);
    handle_cutout(app, model);

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

/// 人物切り抜き（Issue #22）: K キーで再生ヘッド位置の映像クリップに
/// 切り抜きをトグルし、有効化されたらワーカースレッドでオフライン
/// 処理を開始する。完了したら合成済みフレームのテクスチャを読み込み、
/// プレビューが合成結果に切り替わる。
fn handle_cutout(app: &App, model: &mut Model) {
    let k_down = app.keys.down.contains(&Key::K);
    let pressed = k_down && !model.prev_k_down;
    model.prev_k_down = k_down;

    // ジョブの完了・失敗を一度だけ報告し、完了時はテクスチャを読む。
    if let Some(job) = &model.cutout_job {
        let snapshot = job.lock().unwrap().clone();
        if !model.cutout_job_reported {
            match &snapshot.status {
                cutout::JobStatus::Done => {
                    println!(
                        "cutout done: clip {} ({} frames)",
                        snapshot.clip_id, snapshot.total
                    );
                    if let Some((_, clip)) = model.project.clip(snapshot.clip_id) {
                        let dir = media::media_dir().join(cutout::composite_dir_name(&clip.source));
                        for i in 0..snapshot.total {
                            let path = dir.join(format!("f{i:03}.png"));
                            if path.exists() && !model.textures.contains_key(&path) {
                                if let Ok(texture) = wgpu::Texture::from_path(app, &path) {
                                    model.textures.insert(path, texture);
                                }
                            }
                        }
                    }
                    model.cutout_job_reported = true;
                }
                cutout::JobStatus::Failed(e) => {
                    println!("cutout failed: clip {}: {e}", snapshot.clip_id);
                    model.cutout_job_reported = true;
                }
                cutout::JobStatus::Running => {}
            }
        }
    }

    if !pressed {
        return;
    }
    let Some(clip) = media::active_video_clip(&model.project, model.playhead_ms).cloned() else {
        println!("cutout: no video clip at playhead");
        return;
    };
    let enabled = !clip.cutout;
    // 有効化のときは、適用前に処理の前提が揃っているか確認する
    // （レビュー観察: 前提が無いのに cutout: true だけが保存され、
    // 状態表示と実体がずれるのを防ぐ）。無効化はいつでもできる。
    let mut job_inputs = None;
    if enabled {
        let frames = model.frames.frames_for(&clip.source);
        if frames.is_empty() {
            println!("cutout unavailable: no frames for {}", clip.source);
            return;
        }
        let (Some(model_path), Some(dylib_path)) =
            (cutout::find_model(), cutout::find_onnxruntime())
        else {
            println!("cutout unavailable: MODNet model or onnxruntime not found");
            return;
        };
        if model
            .cutout_job
            .as_ref()
            .is_some_and(|j| j.lock().unwrap().status == cutout::JobStatus::Running)
        {
            println!("cutout: a job is already running");
            return;
        }
        job_inputs = Some((frames, model_path, dylib_path));
    }
    let cmd = Command::SetClipCutout {
        clip_id: clip.id,
        enabled,
    };
    if let Err(e) = apply(&mut model.project, &cmd) {
        println!("command rejected: {e}");
        return;
    }
    println!("after cutout toggle: {}", model.project.dump_json());
    save_project_file(model);
    let Some((frames, model_path, dylib_path)) = job_inputs else {
        return;
    };
    // 背景フレーム: 別の映像トラックで同じ時刻を覆うクリップを探す。
    let fg_track = model
        .project
        .clip(clip.id)
        .map(|(t, _)| t.id)
        .unwrap_or_default();
    let bg_frames: Vec<Option<std::path::PathBuf>> = (0..frames.len())
        .map(|i| {
            let timeline_ms = clip.start_ms + (i as i64) * (1000 / model.frames.fps as i64);
            model
                .project
                .tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Video && t.id != fg_track)
                .flat_map(|t| t.clips.iter())
                .find(|c| timeline_ms >= c.start_ms && timeline_ms < c.end_ms())
                .and_then(|bg| {
                    model
                        .frames
                        .frame_path(&bg.source, timeline_ms - bg.start_ms)
                        .map(|p| p.to_path_buf())
                })
        })
        .collect();
    let request = cutout::JobRequest {
        clip_id: clip.id,
        frames,
        bg_frames,
        mask_dir: media::media_dir().join(cutout::mask_dir_name(&clip.source)),
        composite_dir: media::media_dir().join(cutout::composite_dir_name(&clip.source)),
        model_path,
        dylib_path,
    };
    model.cutout_job = Some(cutout::start_job(request));
    model.cutout_job_reported = false;
    println!("cutout started: clip {} ({})", clip.id, clip.name);
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
                        Ok(_) => {
                            println!("placed: {}", model.project.dump_json());
                            save_project_file(model);
                        }
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
        save_project_file(model);
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
            // 切り抜き有効で合成済みフレームがあれば、そちらを優先する。
            let composite_path = if clip.cutout {
                let index = media::frame_index(source_time, model.frames.fps);
                Some(
                    media::media_dir()
                        .join(cutout::composite_dir_name(&clip.source))
                        .join(format!("f{index:03}.png")),
                )
            } else {
                None
            };
            let texture = composite_path
                .as_ref()
                .and_then(|p| model.textures.get(p))
                .or_else(|| {
                    model
                        .frames
                        .frame_path(&clip.source, source_time)
                        .and_then(|p| model.textures.get(p))
                });
            if let Some(texture) = texture {
                draw.texture(texture)
                    .x_y(pvx, pvy)
                    .w_h(PREVIEW_W, PREVIEW_H);
            }
            let label = if clip.cutout {
                format!("{} @ {}ms (cutout)", clip.name, source_time)
            } else {
                format!("{} @ {}ms", clip.name, source_time)
            };
            draw.text(&label)
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

    // 切り抜きジョブの進捗（実行中のみ、プレビューの下に表示する）。
    if let Some(job) = &model.cutout_job {
        let state = job.lock().unwrap();
        if state.status == cutout::JobStatus::Running {
            draw.text(&format!(
                "cutout processing: {}/{}",
                state.done, state.total
            ))
            .x_y(pvx, pvy - PREVIEW_H / 2.0 - 16.0)
            .font_size(11)
            .color(rgb(0.95, 0.75, 0.35));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_file_stamp_matches_dump_stamp() {
        // GUI が保存した直後に監視が「変更あり」と誤判定して再読込
        // しないこと（Issue #11 の自己保存ループ防止）を固定する。
        let project = demo_project();
        let path =
            std::env::temp_dir().join(format!("rust-studio-ve-test-{}.json", std::process::id()));
        project.save_to_path(&path).unwrap();
        let file_bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            media::fnv1a(&file_bytes),
            media::fnv1a(project.dump_json().as_bytes())
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn stamp_distinguishes_different_states() {
        let a = demo_project();
        let mut b = demo_project();
        let clip_id = a
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.name == "interview")
            .unwrap()
            .id;
        apply(
            &mut b,
            &Command::MoveClip {
                clip_id,
                new_start_ms: 100,
            },
        )
        .unwrap();
        assert_ne!(
            media::fnv1a(a.dump_json().as_bytes()),
            media::fnv1a(b.dump_json().as_bytes())
        );
    }
}
