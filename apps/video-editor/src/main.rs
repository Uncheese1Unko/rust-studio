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
mod palette;
mod timeline;

use nannou::prelude::*;
use studio_core::{apply, Command, History, Project, TrackKind};

struct Model {
    _window: WindowId,
    history: History,
    playhead_ms: i64,
    // Issue #9: ポインタ編集の状態。
    drag: Option<edit::Drag>,
    last_drag_cmd: Option<Command>,
    // Issue #31: ドラッグ開始時のスナップショット。履歴への記録は
    // ジェスチャ終了時に行う。
    drag_before: Option<Project>,
    last_rejection: Option<String>,
    prev_left_down: bool,
    clicks: edit::ClickTracker,
    // Issue #57: トラック操作の GUI 状態（ヘッダーのダブルクリック判定、
    // リネーム編集セッション、選択中トラック）。いずれも表示状態で、
    // プロジェクト状態には含めない。
    header_clicks: edit::ClickTracker,
    rename: Option<edit::RenameSession>,
    selected_track: Option<u64>,
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
    // Issue #31: Undo/Redo キーの押下エッジ追跡。
    history_keys: edit::HistoryKeyTracker,
    // Issue #44: 複製・コピー＆ペーストのキー追跡とアプリ内
    // クリップボード（プロジェクトには保存しない）。
    edit_keys: edit::EditKeyTracker,
    clipboard: Option<studio_core::Clip>,
}

/// プレビュー領域（左上原点ピクセル）。要素識別子は `preview`。
const PREVIEW_X: f32 = 500.0;
const PREVIEW_Y: f32 = 150.0;
const PREVIEW_W: f32 = 440.0;
const PREVIEW_H: f32 = 248.0;
const PREVIEW_ID: &str = "preview";

fn main() {
    nannou::app(model).event(event).update(update).run();
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
    let initial_selected = project.tracks.first().map(|t| t.id);
    Model {
        _window: window,
        history: History::new(project),
        playhead_ms: 0,
        drag: None,
        last_drag_cmd: None,
        drag_before: None,
        last_rejection: None,
        prev_left_down: false,
        clicks: edit::ClickTracker::new(),
        header_clicks: edit::ClickTracker::new(),
        rename: None,
        selected_track: initial_selected,
        frames,
        textures,
        last_preview: None,
        project_file,
        file_stamp,
        last_watch_check: 0.0,
        cutout_job: None,
        cutout_job_reported: false,
        prev_k_down: false,
        history_keys: edit::HistoryKeyTracker::new(),
        edit_keys: edit::EditKeyTracker::new(),
        clipboard: None,
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
            model.drag_before = None;
            // リネーム編集も打ち切る（バグ #60: DAW と同じ理由）。
            model.rename = None;
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
            model.history = History::new(new_project);
            if new_sources != old_sources {
                model.frames = media::prepare_media(model.history.project());
                model.textures = load_textures(app, model.history.project(), &model.frames);
            }
            println!("project reloaded: {}", model.history.project().dump_json());
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
    match model.history.project().save_to_path(&path) {
        Ok(()) => {
            model.file_stamp = media::fnv1a(model.history.project().dump_json().as_bytes());
        }
        Err(e) => println!("warning: cannot save project file {}: {e}", path.display()),
    }
}

/// リネーム編集中のキー・文字イベント（Issue #57）。確定は
/// RenameTrack コマンド経由でのみ行い、コアが拒否した場合は編集を
/// 継続して理由を標準出力に残す（DAW #56 と同じ意味論）。
fn event(_app: &App, model: &mut Model, event: Event) {
    if model.rename.is_none() {
        return;
    }
    let Event::WindowEvent {
        simple: Some(window_event),
        ..
    } = event
    else {
        return;
    };
    let input = match window_event {
        WindowEvent::ReceivedCharacter(c) => edit::RenameInput::Char(c),
        WindowEvent::KeyPressed(Key::Back) => edit::RenameInput::Backspace,
        WindowEvent::KeyPressed(Key::Return) => edit::RenameInput::Enter,
        WindowEvent::KeyPressed(Key::Escape) => edit::RenameInput::Escape,
        _ => return,
    };
    let action = model.rename.as_mut().expect("checked above").handle(input);
    match action {
        edit::RenameAction::Continue => {}
        edit::RenameAction::Cancel => {
            model.rename = None;
            println!("rename cancelled");
        }
        edit::RenameAction::Commit(cmd) => match model.history.apply(&cmd) {
            Ok(_) => {
                model.rename = None;
                println!("after rename: {}", model.history.project().dump_json());
                save_project_file(model);
            }
            Err(e) => println!("rename rejected: {e}"),
        },
    }
}

fn update(app: &App, model: &mut Model, update: Update) {
    if model.rename.is_some() {
        // リネーム編集中はポインタ操作と切り抜きキーを受け付けない
        // （Issue #57）。再生ヘッドとプレビューは進み続ける。
        model.prev_left_down = app.mouse.buttons.left().is_down();
        // エッジ検出用の prev は編集中も同期する（バグ #60: 同期しないと
        // K を押したままリネームを確定した直後に切り抜きが誤発する）。
        model.prev_k_down = app.keys.down.contains(&Key::K);
        model.history_keys.sync(
            app.keys.down.contains(&Key::Z),
            app.keys.down.contains(&Key::Y),
        );
        model.edit_keys.sync(
            app.keys.down.contains(&Key::D),
            app.keys.down.contains(&Key::C),
            app.keys.down.contains(&Key::V),
        );
    } else {
        handle_pointer(app, model);
        handle_history_keys(app, model);
        handle_edit_keys(app, model);
        handle_cutout(app, model);
    }
    watch_project_file(app, model);

    let dt_ms = update.since_last.as_millis() as i64;
    let end_ms = timeline::project_end_ms(model.history.project());
    model.playhead_ms = timeline::advance_playhead(model.playhead_ms, dt_ms, end_ms);

    // プレビューの表示フレームが変わったら観測用に記録する（Issue #10）。
    let current =
        media::active_video_clip(model.history.project(), model.playhead_ms).and_then(|clip| {
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

/// Ctrl / Shift の押下判定。修飾キーは `app.keys.mods` ではなく
/// `keys.down` に現れるキー（LControl 等）で判定する。
/// ヘッドレス環境（Xvfb + XTEST）では ModifiersChanged が届かず
/// `keys.mods` が更新されないことを実測したため（Issue #31 の証拠
/// ドライブで確認）。実環境でもこちらで判定できる。
fn ctrl_down(app: &App) -> bool {
    app.keys.down.contains(&Key::LControl) || app.keys.down.contains(&Key::RControl)
}

fn shift_down(app: &App) -> bool {
    app.keys.down.contains(&Key::LShift) || app.keys.down.contains(&Key::RShift)
}

fn alt_down(app: &App) -> bool {
    app.keys.down.contains(&Key::LAlt) || app.keys.down.contains(&Key::RAlt)
}

/// Undo/Redo キー（Issue #31）: Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y。
/// 履歴が実際に動いたときだけ、メディア（フレーム・テクスチャ）と
/// 選択を追従させ、プロジェクトファイルへ保存する。
fn handle_history_keys(app: &App, model: &mut Model) {
    // 発火は Z/Y の押下エッジのみ（HistoryKeyTracker）。組み合わせの
    // レベル判定は Shift の離し順で誤発するため使わない（#31 レビュー）。
    let Some(op) = model.history_keys.update(
        false,
        ctrl_down(app),
        shift_down(app),
        app.keys.down.contains(&Key::Z),
        app.keys.down.contains(&Key::Y),
    ) else {
        return;
    };
    // 進行中ジェスチャ（ドラッグ）への配慮は
    // `edit::apply_history_key` に集約されている（Undo は先に
    // ジェスチャを確定させ、Redo はジェスチャに触れずに試す。
    // #31 レビュー round 3・4 対応）。成功したときだけ後始末する。
    let applied = edit::apply_history_key(&mut model.history, op, &mut [&mut model.drag_before]);
    if !applied {
        return;
    }
    model.drag = None;
    model.last_drag_cmd = None;
    model.last_rejection = None;
    model.drag_before = None;
    if model
        .selected_track
        .is_some_and(|id| model.history.project().track(id).is_none())
    {
        model.selected_track = model.history.project().tracks.first().map(|t| t.id);
    }
    model.frames = media::prepare_media(model.history.project());
    model.textures = load_textures(app, model.history.project(), &model.frames);
    model.last_preview = None;
    let label = match op {
        edit::HistoryOp::Undo => "undo",
        edit::HistoryOp::Redo => "redo",
    };
    println!("after {label}: {}", model.history.project().dump_json());
    save_project_file(model);
}

/// 複製・コピー＆ペースト（Issue #44）: Ctrl+D / Ctrl+C / Ctrl+V。
/// 対象クリップは「選択中トラックの再生ヘッド位置にあるクリップ」
/// （DAW #33 と同一方針・同一キー割り当て）。複製は元クリップの
/// 直後、貼り付けは選択中トラックの再生ヘッド位置に置く。配置は
/// AddClip＋パラメータ設定コマンドで、状態変更はコマンド経由のみ。
/// ドラッグ等のジェスチャ進行中は発火しない。
fn handle_edit_keys(app: &App, model: &mut Model) {
    let Some(op) = model.edit_keys.update(
        false,
        ctrl_down(app),
        app.keys.down.contains(&Key::D),
        app.keys.down.contains(&Key::C),
        app.keys.down.contains(&Key::V),
    ) else {
        return;
    };
    if model.drag.is_some() {
        return;
    }
    let target_clip = model
        .selected_track
        .and_then(|tid| model.history.project().track(tid))
        .and_then(|t| {
            t.clips
                .iter()
                .find(|c| model.playhead_ms >= c.start_ms && model.playhead_ms < c.end_ms())
        })
        .map(|c| c.id);
    match op {
        edit::EditOp::Copy => {
            if let Some(clip_id) = target_clip {
                let (_, clip) = model.history.project().clip(clip_id).expect("found above");
                model.clipboard = Some(clip.clone());
                println!("copied clip: {}", clip.name);
            } else {
                println!("copy: no clip at playhead");
            }
        }
        edit::EditOp::Duplicate => {
            let Some(clip_id) = target_clip else {
                println!("duplicate: no clip at playhead");
                return;
            };
            let placement =
                edit::duplicate_placement(model.history.project(), clip_id).expect("clip exists");
            apply_placement(app, model, &placement, "duplicate");
        }
        edit::EditOp::Paste => {
            let (Some(clipboard), Some(track_id)) = (model.clipboard.clone(), model.selected_track)
            else {
                println!("paste: clipboard is empty or no track selected");
                return;
            };
            let placement = edit::paste_placement(&clipboard, track_id, model.playhead_ms);
            apply_placement(app, model, &placement, "paste");
        }
    }
}

/// 配置計画を「1回の操作」として履歴経由で適用する（Issue #44）。
/// 適用の実体（試行→確定と単一 undo 化）は `edit::apply_placement`。
/// 新しいクリップができたらメディア（フレーム・テクスチャ）を
/// 追従させ、プロジェクトファイルへ保存する。
fn apply_placement(app: &App, model: &mut Model, placement: &edit::ClipPlacement, label: &str) {
    if let Err(e) = edit::apply_placement(&mut model.history, placement) {
        println!("{label} rejected: {e}");
        return;
    }
    model.frames = media::prepare_media(model.history.project());
    model.textures = load_textures(app, model.history.project(), &model.frames);
    model.last_preview = None;
    println!("after {label}: {}", model.history.project().dump_json());
    save_project_file(model);
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
                    if let Some((_, clip)) = model.history.project().clip(snapshot.clip_id) {
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
    let Some(clip) = media::active_video_clip(model.history.project(), model.playhead_ms).cloned()
    else {
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
    if let Err(e) = model.history.apply(&cmd) {
        println!("command rejected: {e}");
        return;
    }
    println!(
        "after cutout toggle: {}",
        model.history.project().dump_json()
    );
    save_project_file(model);
    let Some((frames, model_path, dylib_path)) = job_inputs else {
        return;
    };
    // 背景フレーム: 別の映像トラックで同じ時刻を覆うクリップを探す。
    let fg_track = model
        .history
        .project()
        .clip(clip.id)
        .map(|(t, _)| t.id)
        .unwrap_or_default();
    let bg_frames: Vec<Option<std::path::PathBuf>> = (0..frames.len())
        .map(|i| {
            let timeline_ms = clip.start_ms + (i as i64) * (1000 / model.frames.fps as i64);
            model
                .history
                .project()
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
/// 色選択パネルのクリックをコマンドに変換する（Issue #59）。
/// TRACK 行は選択中トラック、CLIP 行は選択中トラックの再生位置にある
/// クリップに適用する（DAW #58 と同じ対象規則）。状態変更はコマンド
/// 経由のみ（ADR-0004）。
fn apply_palette_click(
    model: &mut Model,
    target: palette::PaletteTarget,
    color: Option<studio_core::ColorTag>,
) {
    let cmd = match target {
        palette::PaletteTarget::Track => {
            let Some(track_id) = model.selected_track else {
                println!("color: no track selected");
                return;
            };
            Command::SetTrackColor { track_id, color }
        }
        palette::PaletteTarget::Clip => {
            let clip_id = model
                .selected_track
                .and_then(|tid| model.history.project().track(tid))
                .and_then(|t| {
                    t.clips
                        .iter()
                        .find(|c| model.playhead_ms >= c.start_ms && model.playhead_ms < c.end_ms())
                })
                .map(|c| c.id);
            let Some(clip_id) = clip_id else {
                println!("color: no clip at playhead on selected track");
                return;
            };
            Command::SetClipColor { clip_id, color }
        }
    };
    match model.history.apply(&cmd) {
        Ok(_) => {
            println!("after color: {}", model.history.project().dump_json());
            save_project_file(model);
        }
        Err(e) => println!("command rejected: {e}"),
    }
}

/// ジェスチャ（ドラッグ）の終了時に、開始時スナップショットを履歴へ
/// 1件だけ積む（Issue #31）。逐次適用は `apply_transient` で履歴に
/// 積まないため、ジェスチャ全体が1回の undo 単位になる。記録を
/// 終了時まで遅らせることで、最終状態と開始状態が同じなら記録自体
/// を省く: 動かさないクリックはもちろん、一度動かして原位置へ
/// 戻したドラッグでも「幽霊 undo」を積んだり redo を消したり
/// しない（#31 レビュー round 2 対応）。記録の実体は
/// `edit::flush_gesture_snapshot` で、終了時はスロットも空にする。
fn finish_gesture_snapshot(history: &mut History, before: &mut Option<Project>) {
    edit::flush_gesture_snapshot(history, before);
    *before = None;
}

fn handle_pointer(app: &App, model: &mut Model) {
    let win = app.window_rect();
    // nannou の中心原点座標を、レイアウトの左上原点ピクセルへ変換する。
    let lx = app.mouse.x + win.w() / 2.0;
    let ly = win.h() / 2.0 - app.mouse.y;
    let left_down = app.mouse.buttons.left().is_down();
    let pointer_ms = edit::x_to_ms(lx, win.w());

    if left_down && !model.prev_left_down {
        // 色選択パネル（Issue #59）: トラック追加エリアの下のパネル。
        // 他の領域とは重ならない。
        if let Some((target, color)) =
            palette::hit_palette(model.history.project().tracks.len(), lx, ly)
        {
            apply_palette_click(model, target, color);
            model.prev_left_down = left_down;
            return;
        }
        // トラック削除（Issue #57）: ヘッダー右端の帯。コマンド経由で
        // 削除し、選択が消えたら同じ位置（なければ最後）のトラックへ
        // 選択を移す。
        if let Some(index) = edit::hit_track_delete(model.history.project(), lx, ly) {
            let track_id = model.history.project().tracks[index].id;
            let cmd = Command::RemoveTrack { track_id };
            match model.history.apply(&cmd) {
                Ok(_) => {
                    if model.selected_track == Some(track_id) {
                        model.selected_track = model
                            .history
                            .project()
                            .tracks
                            .get(index)
                            .or_else(|| model.history.project().tracks.last())
                            .map(|t| t.id);
                    }
                    model.drag = None;
                    model.last_drag_cmd = None;
                    model.drag_before = None;
                    println!(
                        "after track remove: {}",
                        model.history.project().dump_json()
                    );
                    save_project_file(model);
                }
                Err(e) => println!("command rejected: {e}"),
            }
            model.prev_left_down = left_down;
            return;
        }
        // トラック追加（Issue #57）: 最終行直下のヘッダー列。
        // 上半分が映像、下半分が音声。追加したトラックを選択する。
        if let Some(kind) = edit::hit_track_add_area(model.history.project(), lx, ly) {
            let cmd = edit::add_track_command(model.history.project(), kind);
            match model.history.apply(&cmd) {
                Ok(created) => {
                    model.selected_track = created;
                    println!("after track add: {}", model.history.project().dump_json());
                    save_project_file(model);
                }
                Err(e) => println!("command rejected: {e}"),
            }
            model.prev_left_down = left_down;
            return;
        }
        // トラックヘッダーのクリックで選択（Issue #57、表示状態のみ）。
        // ダブルクリックでリネーム編集に入る。
        if let Some(index) = edit::hit_track_header(model.history.project(), lx, ly) {
            model.selected_track = Some(model.history.project().tracks[index].id);
            let now_ms = (app.time * 1_000.0) as u64;
            if model.header_clicks.register(now_ms, lx, ly) {
                let track = &model.history.project().tracks[index];
                model.rename = Some(edit::RenameSession::begin(track.id, &track.name));
                println!("rename begin: track {}", track.id);
            }
        }
        let target = edit::hit_test(model.history.project(), win.w(), lx, ly);
        if let Some(drag) = edit::Drag::begin(model.history.project(), &target, pointer_ms) {
            model.drag = Some(drag);
            model.last_drag_cmd = None;
            model.drag_before = Some(model.history.project().clone());
            model.last_rejection = None;
        } else if let edit::HitTarget::Empty { track_index } = target {
            let now_ms = (app.time * 1_000.0) as u64;
            if model.clicks.register(now_ms, lx, ly) {
                if let Some(cmd) =
                    edit::place_clip_command(model.history.project(), track_index, pointer_ms)
                {
                    match model.history.apply(&cmd) {
                        Ok(_) => {
                            println!("placed: {}", model.history.project().dump_json());
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
            // Issue #45: 候補コマンドにスナップを適用する。
            // Alt 押下中はスナップ無効。
            let cmd = drag.snapped_command_for(
                model.history.project(),
                pointer_ms,
                model.playhead_ms,
                win.w(),
                !alt_down(app),
            );
            if model.last_drag_cmd.as_ref() != Some(&cmd) {
                match model.history.apply_transient(&cmd) {
                    Ok(_) => {
                        model.last_drag_cmd = Some(cmd);
                    }
                    Err(e) => model.last_rejection = Some(e.to_string()),
                }
            }
        }
    }

    if !left_down && model.prev_left_down && model.drag.take().is_some() {
        // ジェスチャ終了時にだけ履歴へ記録する（Issue #31）。
        finish_gesture_snapshot(&mut model.history, &mut model.drag_before);
        println!("after drag: {}", model.history.project().dump_json());
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
    for (index, track) in model.history.project().tracks.iter().enumerate() {
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
        // 選択中トラックのヘッダーは少し明るくする（Issue #57）。
        let header_shade = if model.selected_track == Some(track.id) {
            0.26
        } else {
            0.17
        };
        draw.rect()
            .x_y(hx, hy)
            .w_h(timeline::HEADER_WIDTH_PX, timeline::TRACK_HEIGHT_PX)
            .color(rgb(header_shade, header_shade, header_shade + 0.03));
        // トラック色（Issue #59）: ヘッダー左端に表示色の帯を出す。
        // 未設定なら種別色、設定があればパレット色（track_display_color）。
        {
            let (r, g, b) = timeline::track_display_color(track.color, track.kind);
            let (bar_x, bar_y) = rect_center(win, 0.0, top, 6.0, timeline::TRACK_HEIGHT_PX);
            draw.rect()
                .x_y(bar_x, bar_y)
                .w_h(6.0, timeline::TRACK_HEIGHT_PX)
                .color(rgb(r, g, b));
        }
        // リネーム編集中のトラックは編集中文字列＋キャレットを出す
        // （Issue #57）。名前は削除帯を除いた範囲の中央に置く。
        let renaming_this = model.rename.as_ref().filter(|s| s.track_id == track.id);
        let name_text = match renaming_this {
            Some(session) => format!("{}_", session.text),
            None => track.name.clone(),
        };
        let (nx, ny) = rect_center(
            win,
            0.0,
            top,
            timeline::HEADER_WIDTH_PX - edit::DELETE_STRIP_PX,
            timeline::TRACK_HEIGHT_PX,
        );
        draw.text(&name_text)
            .x_y(nx, ny)
            .font_size(13)
            .color(rgb(0.85, 0.85, 0.88));
        // 削除ボタン（Issue #57）: ヘッダー右端の帯に「×」。
        let (xx, xy) = rect_center(
            win,
            timeline::HEADER_WIDTH_PX - edit::DELETE_STRIP_PX,
            top,
            edit::DELETE_STRIP_PX,
            timeline::TRACK_HEIGHT_PX,
        );
        draw.text("×")
            .x_y(xx, xy)
            .font_size(12)
            .color(rgb(0.55, 0.55, 0.60));
    }

    // トラック追加エリア（Issue #57）: 最終行直下のヘッダー列を
    // 上下に分け、上に「+ VIDEO」、下に「+ AUDIO」。
    {
        let top = timeline::track_top_y(model.history.project().tracks.len());
        let half = timeline::TRACK_HEIGHT_PX / 2.0;
        for (i, label) in ["+ VIDEO", "+ AUDIO"].iter().enumerate() {
            let (cx, cy) = rect_center(
                win,
                0.0,
                top + half * i as f32,
                timeline::HEADER_WIDTH_PX,
                half,
            );
            draw.rect()
                .x_y(cx, cy)
                .w_h(timeline::HEADER_WIDTH_PX, half)
                .color(rgb(0.13, 0.13, 0.16));
            draw.text(label)
                .x_y(cx, cy)
                .font_size(10)
                .color(rgb(0.65, 0.65, 0.70));
        }
    }

    // 色選択パネル（Issue #59）: トラック追加エリアの下。TRACK 行は
    // 選択中トラック、CLIP 行は選択中トラックの再生位置にあるクリップ
    // が対象。現在の色のスウォッチには明るい枠を付ける。
    {
        let count = model.history.project().tracks.len();
        let top = palette::panel_top(count);
        let panel_h = palette::panel_height() + 8.0;
        let (px, py) = rect_center(win, 8.0, top - 8.0, 380.0, panel_h);
        draw.rect()
            .x_y(px, py)
            .w_h(380.0, panel_h)
            .color(rgb(0.13, 0.13, 0.16));
        let (lx, ly) = rect_center(win, 16.0, top - 6.0, 60.0, 10.0);
        draw.text("COLOR")
            .x_y(lx, ly)
            .font_size(9)
            .color(rgb(0.65, 0.65, 0.70));
        let selected_track = model
            .selected_track
            .and_then(|id| model.history.project().track(id));
        let playhead_clip = selected_track.and_then(|t| {
            t.clips
                .iter()
                .find(|c| model.playhead_ms >= c.start_ms && model.playhead_ms < c.end_ms())
        });
        for (target, label, current) in [
            (
                palette::PaletteTarget::Track,
                "TRACK",
                selected_track.and_then(|t| t.color),
            ),
            (
                palette::PaletteTarget::Clip,
                "CLIP",
                playhead_clip.and_then(|c| c.color),
            ),
        ] {
            let row_y = top
                + palette::ROWS_TOP_OFFSET_PX
                + match target {
                    palette::PaletteTarget::Track => 0.0,
                    palette::PaletteTarget::Clip => palette::ROW_H_PX,
                };
            let (tx, ty) = rect_center(
                win,
                8.0,
                row_y,
                palette::ROW_LABEL_W_PX + 8.0,
                palette::ROW_H_PX,
            );
            draw.text(label)
                .x_y(tx, ty)
                .font_size(10)
                .color(rgb(0.65, 0.65, 0.70));
            for index in 0..=timeline::PALETTE.len() {
                let rect = palette::swatch_rect(top, target, index);
                let is_current = match index {
                    0 => current.is_none(),
                    i => current == Some(timeline::PALETTE[i - 1].0),
                };
                let (cx, cy) = rect_center(win, rect.x, rect.y, rect.w, rect.h);
                if is_current {
                    draw.rect()
                        .x_y(cx, cy)
                        .w_h(rect.w + 4.0, rect.h + 4.0)
                        .color(rgb(0.90, 0.90, 0.92));
                }
                if index == 0 {
                    // 「なし」: 暗い面＋×（未設定に戻す）
                    draw.rect()
                        .x_y(cx, cy)
                        .w_h(rect.w, rect.h)
                        .color(rgb(0.20, 0.20, 0.24));
                    draw.text("×")
                        .x_y(cx, cy)
                        .font_size(11)
                        .color(rgb(0.65, 0.65, 0.70));
                } else {
                    let (r, g, b) = timeline::PALETTE[index - 1].1;
                    draw.rect()
                        .x_y(cx, cy)
                        .w_h(rect.w, rect.h)
                        .color(rgb(r, g, b));
                }
            }
        }
    }

    // クリップ（timeline.clip.<id>）: コアの start/duration をそのまま矩形にする。
    for rect in timeline::layout_clips(model.history.project(), win.w()) {
        let clip = model.history.project().clip(rect.clip_id).map(|(_, c)| c);
        // 表示色（Issue #59）: ユーザー色があればそれ、なければ種別色。
        let (r, g, b) = timeline::clip_display_color(clip.and_then(|c| c.color), rect.kind);
        let (cx, cy) = rect_center(win, rect.x, rect.y + 4.0, rect.w, rect.h - 8.0);
        draw.rect()
            .x_y(cx, cy)
            .w_h(rect.w, rect.h - 8.0)
            .color(rgb(r, g, b));
        if let Some(clip) = clip {
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
    match media::active_video_clip(model.history.project(), model.playhead_ms) {
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
    fn gesture_snapshot_skips_unchanged_state() {
        // Issue #31 レビュー対応: 動かさないクリック（無変化の逐次適用）
        // ではスナップショットは積まれず、redo も消えない。実際に
        // 変わった後の適用では1件だけ積まれる。
        let mut history = History::new(demo_project());
        let clip_id = history.project().tracks[0].clips[0].id;
        let origin = history.project().tracks[0].clips[0].start_ms;
        // redo を用意する: 適用→undo で redo が積まれた状態にする
        history
            .apply(&Command::MoveClip {
                clip_id,
                new_start_ms: 500,
            })
            .unwrap();
        assert!(history.undo());
        assert!(history.can_redo());
        // 無変化の逐次適用（同じ位置への MoveClip）→ 終了時の記録は
        // 省かれ、redo も消えない。
        let mut before = Some(history.project().clone());
        history
            .apply_transient(&Command::MoveClip {
                clip_id,
                new_start_ms: history.project().tracks[0].clips[0].start_ms,
            })
            .unwrap();
        finish_gesture_snapshot(&mut history, &mut before);
        assert!(!history.can_undo());
        assert!(history.can_redo());
        // 実際に動かしたジェスチャは、終了時に1件だけ積まれる。
        let mut before = Some(history.project().clone());
        history
            .apply_transient(&Command::MoveClip {
                clip_id,
                new_start_ms: 700,
            })
            .unwrap();
        finish_gesture_snapshot(&mut history, &mut before);
        assert!(history.can_undo());
        assert!(!history.can_redo());
        assert!(history.undo());
        assert_eq!(history.project().tracks[0].clips[0].start_ms, origin);
    }

    #[test]
    fn gesture_returned_to_origin_records_nothing() {
        // Issue #31 レビュー round 2 対応: 一度動かして原位置へ戻した
        // ドラッグは、終了時の最終状態が開始状態と同じなので記録
        // されない（幽霊 undo を積まず、redo も消さない）。
        let mut history = History::new(demo_project());
        let clip_id = history.project().tracks[0].clips[0].id;
        // redo を用意する
        history
            .apply(&Command::MoveClip {
                clip_id,
                new_start_ms: 500,
            })
            .unwrap();
        assert!(history.undo());
        assert!(history.can_redo());
        let origin = history.project().tracks[0].clips[0].start_ms;
        let mut before = Some(history.project().clone());
        // 動かしてから原位置へ戻す
        history
            .apply_transient(&Command::MoveClip {
                clip_id,
                new_start_ms: 700,
            })
            .unwrap();
        history
            .apply_transient(&Command::MoveClip {
                clip_id,
                new_start_ms: origin,
            })
            .unwrap();
        finish_gesture_snapshot(&mut history, &mut before);
        assert!(!history.can_undo());
        assert!(history.can_redo());
        assert!(!history.undo());
    }

    #[test]
    fn undo_during_gesture_consumes_the_gesture_itself() {
        // Issue #31 レビュー round 3 対応: ジェスチャの途中で undo
        // する場合、`handle_history_keys` は履歴操作の前にジェスチャを
        // 確定させる。そのため消費されるのは直前の確定編集ではなく
        // ジェスチャ自身であり、確定状態（500）は失われず、redo で
        // 途中状態（900）へ進める。確定させないと undo は確定編集を
        // 消費して開始状態に戻り、500 が復元不能になる。
        let mut history = History::new(demo_project());
        let clip_id = history.project().tracks[0].clips[0].id;
        // 確定編集: → 500
        history
            .apply(&Command::MoveClip {
                clip_id,
                new_start_ms: 500,
            })
            .unwrap();
        // ジェスチャ開始: スナップショットを控え、逐次適用で 900 へ
        let mut before = Some(history.project().clone());
        history
            .apply_transient(&Command::MoveClip {
                clip_id,
                new_start_ms: 900,
            })
            .unwrap();
        // handle_history_keys が使う純粋関数そのもので検証する:
        // Undo はジェスチャを確定させてから消費する
        assert!(edit::apply_history_key(
            &mut history,
            edit::HistoryOp::Undo,
            &mut [&mut before],
        ));
        assert_eq!(history.project().tracks[0].clips[0].start_ms, 500);
        assert!(edit::apply_history_op(&mut history, edit::HistoryOp::Redo));
        assert_eq!(history.project().tracks[0].clips[0].start_ms, 900);
    }

    #[test]
    fn redo_during_changed_gesture_fails_without_losing_the_gesture() {
        // Issue #31 レビュー round 4 対応: 変化済みジェスチャの途中で
        // Redo を試しても、redo スタックが空なら失敗するだけで、
        // ジェスチャのスナップショットも途中状態も失われない。
        // ジェスチャは継続でき、終了時に通常どおり記録される。
        // （旧実装は確定→redo の順で、確定が redo を消して必ず失敗し、
        // スナップショットだけ消費していた。）
        let mut history = History::new(demo_project());
        let clip_id = history.project().tracks[0].clips[0].id;
        let origin = history.project().tracks[0].clips[0].start_ms;
        let mut before = Some(history.project().clone());
        history
            .apply_transient(&Command::MoveClip {
                clip_id,
                new_start_ms: 900,
            })
            .unwrap();
        assert!(!edit::apply_history_key(
            &mut history,
            edit::HistoryOp::Redo,
            &mut [&mut before],
        ));
        assert!(before.is_some());
        assert_eq!(history.project().tracks[0].clips[0].start_ms, 900);
        assert!(!history.can_undo());
        // ジェスチャ終了時に記録され、undo で開始位置へ戻れる
        finish_gesture_snapshot(&mut history, &mut before);
        assert!(history.can_undo());
        assert!(history.undo());
        assert_eq!(history.project().tracks[0].clips[0].start_ms, origin);
    }

    #[test]
    fn failed_undo_during_unchanged_gesture_keeps_the_gesture_armed() {
        // Issue #31 レビュー round 4 対応: 未変化ジェスチャの途中で
        // Undo が空振り（履歴が空）しても、スナップショットは消費
        // されず、その後の動きが終了時に記録される。
        let mut history = History::new(demo_project());
        let clip_id = history.project().tracks[0].clips[0].id;
        let origin = history.project().tracks[0].clips[0].start_ms;
        let mut before = Some(history.project().clone());
        assert!(!edit::apply_history_key(
            &mut history,
            edit::HistoryOp::Undo,
            &mut [&mut before],
        ));
        assert!(before.is_some());
        // そのままドラッグを続けて離すと記録される
        history
            .apply_transient(&Command::MoveClip {
                clip_id,
                new_start_ms: 900,
            })
            .unwrap();
        finish_gesture_snapshot(&mut history, &mut before);
        assert!(history.can_undo());
        assert!(history.undo());
        assert_eq!(history.project().tracks[0].clips[0].start_ms, origin);
    }

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
