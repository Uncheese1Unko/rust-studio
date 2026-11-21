//! daw: Reaper 風の DAW（nannou）。
//!
//! Issue #13 で音声再生エンジン（`audio`）、Issue #14 でタイムライン UI と
//! クリップ配置・移動・トリム、波形表示を実装した。操作の意味論は
//! video-editor と同じ（コアのコマンドに揃える）。
//! 状態の変更は studio-core のコマンド層経由でのみ行う（ADR-0004）。
//! 実デバイスへの音声出力は実機未検証（エンジンのレンダリングと状態は
//! テストで固定済み）。
//!
//! 起動モード:
//! - 既定: GUI（タイムライン）
//! - `--engine-demo`: #13 のコンソールデモ（ミックス書き出しと再生遷移）

mod audio;
mod edit;
mod mixer;
mod sampler;
mod timeline;

use std::collections::HashMap;

use nannou::prelude::*;
use studio_core::{apply, Command, Project, TrackKind};

const RATE: u32 = 44100;
/// 波形ピークの事前計算本数（ソース全体に対して）。
const WAVEFORM_COLUMNS: usize = 1024;

fn media_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("rust-studio-daw-media")
}

fn write_tone(path: &std::path::Path, freq: f32, ms: i64) {
    if path.exists() {
        return;
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    let frames = RATE as i64 * ms / 1000;
    for i in 0..frames {
        let s = 0.4 * (2.0 * std::f32::consts::PI * freq * i as f32 / RATE as f32).sin();
        writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
    }
    writer.finalize().unwrap();
}

/// デモ素材を用意し、パスを返す（220Hz / 440Hz / 配置用 330Hz）。
fn prepare_demo_media() -> (String, String, String) {
    let dir = media_dir();
    std::fs::create_dir_all(&dir).unwrap();
    write_tone(&dir.join("tone220.wav"), 220.0, 2500);
    write_tone(&dir.join("tone440.wav"), 440.0, 2500);
    write_tone(&dir.join("demo-dropped.wav"), 330.0, 2500);
    (
        dir.join("tone220.wav").to_string_lossy().into_owned(),
        dir.join("tone440.wav").to_string_lossy().into_owned(),
        dir.join("demo-dropped.wav").to_string_lossy().into_owned(),
    )
}

/// デモプロジェクトはコマンド経由でのみ組み立てる（ADR-0004）。
fn demo_project(tone220: &str, tone440: &str) -> Project {
    let mut project = Project::new("daw demo session");
    let track = apply(
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
            track_id: track,
            name: "tone-220".into(),
            start_ms: 0,
            duration_ms: 2000,
            source: tone220.into(),
        },
    )
    .unwrap();
    apply(
        &mut project,
        &Command::AddClip {
            track_id: track,
            name: "tone-440".into(),
            start_ms: 2500,
            duration_ms: 2000,
            source: tone440.into(),
        },
    )
    .unwrap();
    project
}

struct Model {
    _window: WindowId,
    project: Project,
    playhead_ms: i64,
    drag: Option<edit::Drag>,
    last_drag_cmd: Option<Command>,
    last_rejection: Option<String>,
    prev_left_down: bool,
    clicks: edit::ClickTracker,
    /// ソースごとの波形ピーク（全体を WAVEFORM_COLUMNS 本で事前計算）。
    waveforms: HashMap<String, Vec<f32>>,
    /// ソースごとの長さ（ms）。波形の切り出しに使う。
    source_durations: HashMap<String, i64>,
    dropped_source: String,
    /// 操作中のミキサースライダー（トラック ID とパラメータ）。
    mixer_active: Option<(u64, mixer::Param)>,
    last_mixer_cmd: Option<Command>,
    // Issue #16: プロジェクトファイル共有モードの状態（#11 と同じ方式）。
    project_file: Option<std::path::PathBuf>,
    file_stamp: u64,
    last_watch_check: f32,
    // Issue #18: ピッチ操作キー（↑↓）の押下エッジ検出。
    prev_pitch_up: bool,
    prev_pitch_down: bool,
    // Issue #19: サンプラー（素材は初回ノートオン時に確定する）。
    sampler: Option<sampler::Sampler>,
    prev_note_keys: [bool; 13],
}

/// プロジェクトファイル共有モード（Issue #16、ADR-0010。video-editor の
/// #11 と同じ方式）の入口。環境変数 `STUDIO_PROJECT_FILE` があればその
/// ファイルのプロジェクトを、なければデモプロジェクトを返す。
fn initial_project(tone220: &str, tone440: &str) -> (Project, Option<std::path::PathBuf>, u64) {
    let path = std::env::var_os("STUDIO_PROJECT_FILE").map(std::path::PathBuf::from);
    if let Some(path) = path {
        match Project::load_from_path(&path) {
            Ok(project) => {
                let stamp = std::fs::read(&path).map(|bytes| fnv1a(&bytes)).unwrap_or(0);
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
    (demo_project(tone220, tone440), None, 0)
}

/// FNV-1a 64bit（ファイル変更検出用。video-editor と同じ実装）。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// プロジェクトのクリップが参照するソース一覧（ソート・重複なし）。
/// 波形マップのキーとは別物で、配置用素材は含まない（Issue #16 の
/// レビュー指摘: 再読込時の比較はこの一覧同士で行う）。
fn project_sources(project: &Project) -> Vec<String> {
    let mut sources: Vec<String> = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.source.clone()))
        .collect();
    sources.sort();
    sources.dedup();
    sources
}

/// 2つのプロジェクトでソース集合が変わったか。
fn sources_changed(old: &Project, new: &Project) -> bool {
    project_sources(old) != project_sources(new)
}

/// 波形と長さを再計算し、配置用素材（プロジェクトに現れないソース）
/// の分も保持する。
fn refresh_waveforms(
    project: &Project,
    dropped_source: &str,
    waveforms: &mut HashMap<String, Vec<f32>>,
    durations: &mut HashMap<String, i64>,
) {
    let (mut new_waveforms, mut new_durations) = compute_waveforms(project);
    if !new_waveforms.contains_key(dropped_source) {
        if let Ok(decoded) = audio::decode_audio(std::path::Path::new(dropped_source)) {
            new_waveforms.insert(
                dropped_source.to_string(),
                timeline::waveform_peaks(
                    &decoded.samples,
                    decoded.channels as usize,
                    WAVEFORM_COLUMNS,
                ),
            );
            new_durations.insert(dropped_source.to_string(), decoded.duration_ms());
        }
    }
    *waveforms = new_waveforms;
    *durations = new_durations;
}

/// プロジェクトのソース集合に対する波形と長さを計算する。
fn compute_waveforms(project: &Project) -> (HashMap<String, Vec<f32>>, HashMap<String, i64>) {
    let mut waveforms = HashMap::new();
    let mut durations = HashMap::new();
    let mut sources: Vec<&str> = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.source.as_str()))
        .collect();
    sources.sort();
    sources.dedup();
    for source in sources {
        if let Ok(decoded) = audio::decode_audio(std::path::Path::new(source)) {
            waveforms.insert(
                source.to_string(),
                timeline::waveform_peaks(
                    &decoded.samples,
                    decoded.channels as usize,
                    WAVEFORM_COLUMNS,
                ),
            );
            durations.insert(source.to_string(), decoded.duration_ms());
        }
    }
    (waveforms, durations)
}

fn model(app: &App) -> Model {
    let (tone220, tone440, dropped) = prepare_demo_media();
    let (project, project_file, file_stamp) = initial_project(&tone220, &tone440);
    println!("daw (issue #14: timeline UI)");
    println!("{}", project.dump_json());
    println!("elements: {}", timeline::element_ids(&project).join(", "));
    let (mut waveforms, mut source_durations) = (HashMap::new(), HashMap::new());
    // 配置用素材も波形を用意しておく（配置直後から波形が出るように）
    refresh_waveforms(&project, &dropped, &mut waveforms, &mut source_durations);
    let window = app
        .new_window()
        .size(960, 540)
        .title("daw")
        .view(view)
        .build()
        .unwrap();
    Model {
        _window: window,
        project,
        playhead_ms: 0,
        drag: None,
        last_drag_cmd: None,
        last_rejection: None,
        prev_left_down: false,
        clicks: edit::ClickTracker::new(),
        waveforms,
        source_durations,
        dropped_source: dropped,
        mixer_active: None,
        last_mixer_cmd: None,
        project_file,
        file_stamp,
        last_watch_check: 0.0,
        prev_pitch_up: false,
        prev_pitch_down: false,
        sampler: None,
        prev_note_keys: [false; 13],
    }
}

fn main() {
    if std::env::args().any(|a| a == "--engine-demo") {
        engine_demo();
        return;
    }
    // ピッチ処理の単体実行（Issue #18 の音声証拠生成用）:
    // daw --pitch-demo <in.wav> <semitones> <out.wav>
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 5 && args[1] == "--pitch-demo" {
        pitch_demo(&args[2], &args[3], &args[4]);
        return;
    }
    // サンプラーの和音レンダリング（Issue #19 の音声証拠生成用）:
    // daw --sampler-demo <in.wav> <out_dir>
    if args.len() == 4 && args[1] == "--sampler-demo" {
        sampler_demo(&args[2], &args[3]);
        return;
    }
    nannou::app(model).update(update).run();
}

/// 素材をサンプラーに読み、和音（0/+4/+7）と単音（+7）を書き出す。
fn sampler_demo(input: &str, out_dir: &str) {
    let decoded = audio::decode_audio(std::path::Path::new(input)).expect("decode input wav");
    std::fs::create_dir_all(out_dir).expect("create out dir");
    let write = |name: &str, samples: &[f32], rate: u32| {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let path = format!("{out_dir}/{name}");
        let mut writer = hound::WavWriter::create(&path, spec).expect("create wav");
        for s in samples {
            writer
                .write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .expect("write sample");
        }
        writer.finalize().expect("finalize wav");
        println!("sampler demo: wrote {path}");
    };
    let rate = decoded.sample_rate;
    let frames = rate as usize; // 1 秒
    let mut chord = sampler::Sampler::new(input.to_string(), decoded.clone());
    chord.note_on(0);
    chord.note_on(4);
    chord.note_on(7);
    println!("sampler demo: active {:?}", chord.active_notes());
    let mixed = chord.render(frames);
    write("chord.wav", &mixed, rate);
    let mut single = sampler::Sampler::new(input.to_string(), decoded);
    single.note_on(7);
    let mixed = single.render(frames);
    write("note-plus7.wav", &mixed, rate);
}

/// WAV を読み、ピッチ変更して書き出す（エンジンと同じ処理経路）。
fn pitch_demo(input: &str, semitones: &str, output: &str) {
    let semitones: f32 = semitones.parse().expect("semitones must be a number");
    let decoded = audio::decode_audio(std::path::Path::new(input)).expect("decode input wav");
    let shifted = audio::pitch_shift(&decoded, semitones);
    let spec = hound::WavSpec {
        channels: shifted.channels,
        sample_rate: shifted.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output, spec).expect("create output wav");
    for s in &shifted.samples {
        writer
            .write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .expect("write sample");
    }
    writer.finalize().expect("finalize wav");
    println!("pitch demo: {input} {semitones:+} st -> {output}");
}

/// プロジェクトファイルの変更を監視し、変わっていたら読み直す
/// （Issue #16、video-editor の #11 と同じ方式）。読み込みに失敗した
/// 場合は現在の状態を保ち、次回に再試行する。
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
    let stamp = fnv1a(&bytes);
    if stamp == model.file_stamp {
        return;
    }
    match Project::load_from_path(&path) {
        Ok(new_project) => {
            model.file_stamp = stamp;
            // 外部変更で状態が差し替わるため、旧状態前提の操作は打ち切る
            model.drag = None;
            model.last_drag_cmd = None;
            model.last_rejection = None;
            model.mixer_active = None;
            model.last_mixer_cmd = None;
            // ソース集合の比較はプロジェクトのクリップ同士で行い、
            // 再計算では配置用素材の波形を保持する（Issue #16 レビュー指摘）。
            let changed = sources_changed(&model.project, &new_project);
            model.project = new_project;
            if changed {
                refresh_waveforms(
                    &model.project,
                    &model.dropped_source,
                    &mut model.waveforms,
                    &mut model.source_durations,
                );
            }
            println!("project reloaded: {}", model.project.dump_json());
        }
        Err(e) => {
            println!("warning: project reload failed: {e}");
        }
    }
}

/// GUI の編集結果をプロジェクトファイルへ保存し直す（Issue #16）。
/// 自分の保存で監視が再読込しないよう、スタンプも同期させる。
fn save_project_file(model: &mut Model) {
    let Some(path) = model.project_file.clone() else {
        return;
    };
    match model.project.save_to_path(&path) {
        Ok(()) => {
            model.file_stamp = fnv1a(model.project.dump_json().as_bytes());
        }
        Err(e) => println!("warning: cannot save project file {}: {e}", path.display()),
    }
}

fn update(app: &App, model: &mut Model, update: Update) {
    handle_pointer(app, model);
    watch_project_file(app, model);
    handle_pitch_keys(app, model);
    let dt_ms = (update.since_last.as_secs_f64() * 1000.0) as i64;
    handle_sampler_keys(app, model);
    if let Some(sampler) = &mut model.sampler {
        sampler.advance(dt_ms);
    }
    let end = timeline::project_end_ms(&model.project);
    model.playhead_ms = timeline::advance_playhead(model.playhead_ms, dt_ms, end);
}

/// サンプラーのキー配置（sampler::key_to_semitone と対応）。
const NOTE_KEYS: [(Key, char); 13] = [
    (Key::A, 'a'),
    (Key::W, 'w'),
    (Key::S, 's'),
    (Key::E, 'e'),
    (Key::D, 'd'),
    (Key::F, 'f'),
    (Key::T, 't'),
    (Key::G, 'g'),
    (Key::Y, 'y'),
    (Key::H, 'h'),
    (Key::U, 'u'),
    (Key::J, 'j'),
    (Key::K, 'k'),
];

/// サンプラー演奏（Issue #19）: ピアノ式キー配置で、再生ヘッド位置の
/// クリップの素材を音程で鳴らす。ノートオン／オフは操作ログに出す。
fn handle_sampler_keys(app: &App, model: &mut Model) {
    for (idx, (key, ch)) in NOTE_KEYS.iter().enumerate() {
        let down = app.keys.down.contains(key);
        let was = model.prev_note_keys[idx];
        model.prev_note_keys[idx] = down;
        let Some(semitones) = sampler::key_to_semitone(*ch) else {
            continue;
        };
        if down && !was {
            if model.sampler.is_none() {
                // 素材は再生ヘッド位置の音声クリップ、無ければ配置用素材。
                let source = model
                    .project
                    .tracks
                    .iter()
                    .filter(|t| t.kind == studio_core::TrackKind::Audio)
                    .flat_map(|t| t.clips.iter())
                    .find(|c| model.playhead_ms >= c.start_ms && model.playhead_ms < c.end_ms())
                    .map(|c| c.source.clone())
                    .unwrap_or_else(|| model.dropped_source.clone());
                match audio::decode_audio(std::path::Path::new(&source)) {
                    Ok(decoded) => {
                        println!("sampler source: {source}");
                        model.sampler = Some(sampler::Sampler::new(source, decoded));
                    }
                    Err(e) => {
                        println!("sampler unavailable: cannot decode {source}: {e}");
                        return;
                    }
                }
            }
            if let Some(sampler) = &mut model.sampler {
                sampler.note_on(semitones);
                println!(
                    "note on: {:+}, active: {:?}",
                    semitones,
                    sampler.active_notes()
                );
            }
        } else if !down && was {
            if let Some(sampler) = &mut model.sampler {
                sampler.note_off(semitones);
                println!(
                    "note off: {:+}, active: {:?}",
                    semitones,
                    sampler.active_notes()
                );
            }
        }
    }
}

/// ピッチ操作（Issue #18）: ↑↓ キーで再生ヘッド位置の音声クリップの
/// ピッチを ±1 半音変える。変更はコマンド経由で、ダンプに反映される。
fn handle_pitch_keys(app: &App, model: &mut Model) {
    let up = app.keys.down.contains(&Key::Up);
    let down = app.keys.down.contains(&Key::Down);
    let delta = match (up && !model.prev_pitch_up, down && !model.prev_pitch_down) {
        (true, false) => 1.0,
        (false, true) => -1.0,
        _ => 0.0,
    };
    model.prev_pitch_up = up;
    model.prev_pitch_down = down;
    if delta == 0.0 {
        return;
    }
    let Some((_, clip)) = model.project.tracks.iter().find_map(|t| {
        if t.kind != studio_core::TrackKind::Audio {
            return None;
        }
        t.clips
            .iter()
            .find(|c| model.playhead_ms >= c.start_ms && model.playhead_ms < c.end_ms())
            .map(|c| (t, c))
    }) else {
        println!("pitch: no audio clip at playhead");
        return;
    };
    let cmd = Command::SetClipPitch {
        clip_id: clip.id,
        semitones: clip.pitch_semitones + delta,
    };
    match apply(&mut model.project, &cmd) {
        Ok(_) => {
            println!("after pitch: {}", model.project.dump_json());
            save_project_file(model);
        }
        Err(e) => println!("command rejected: {e}"),
    }
}

/// ポインタ操作をポーリングし、コマンド経由で編集する（Issue #14、
/// video-editor と同じ意味論）。押下でドラッグ開始（またはダブルクリック
/// で配置）、ドラッグ中は候補コマンドが変わるたびに適用し、解放時に
/// 状態ダンプを出す。範囲外の候補はコマンド層が弾く。
fn handle_pointer(app: &App, model: &mut Model) {
    let win = app.window_rect();
    // nannou の中心原点座標を、レイアウトの左上原点ピクセルへ変換する。
    let lx = app.mouse.x + win.w() / 2.0;
    let ly = win.h() / 2.0 - app.mouse.y;
    let left_down = app.mouse.buttons.left().is_down();
    let pointer_ms = edit::x_to_ms(lx, win.w());

    // ミキサー（Issue #15）: スライダーの押下・ドラッグで音量／パンを
    // コマンド経由で変える。タイムライン領域とは重ならない。
    if left_down && !model.prev_left_down {
        if let Some(hit) = mixer::hit_mixer(&model.project, lx, ly) {
            model.mixer_active = Some((hit.track_id, hit.param));
            let cmd = mixer::command_for(&hit);
            if apply(&mut model.project, &cmd).is_ok() {
                model.last_mixer_cmd = Some(cmd);
            }
        }
    }
    if left_down {
        if let Some((track_id, param)) = model.mixer_active {
            if let Some(hit) = mixer::hit_with_x(&model.project, track_id, param, lx) {
                let cmd = mixer::command_for(&hit);
                if model.last_mixer_cmd.as_ref() != Some(&cmd) {
                    match apply(&mut model.project, &cmd) {
                        Ok(_) => model.last_mixer_cmd = Some(cmd),
                        Err(e) => println!("command rejected: {e}"),
                    }
                }
            }
        }
    }
    if !left_down && model.prev_left_down && model.mixer_active.take().is_some() {
        println!("after mixer: {}", model.project.dump_json());
        save_project_file(model);
        model.last_mixer_cmd = None;
    }

    if left_down && !model.prev_left_down {
        let target = edit::hit_test(&model.project, win.w(), lx, ly);
        if let Some(drag) = edit::Drag::begin(&model.project, &target, pointer_ms) {
            model.drag = Some(drag);
            model.last_drag_cmd = None;
            model.last_rejection = None;
        } else if let edit::HitTarget::Empty { track_index } = target {
            let now_ms = (app.time * 1_000.0) as u64;
            if model.clicks.register(now_ms, lx, ly) {
                if let Some(cmd) = edit::place_clip_command(
                    &model.project,
                    track_index,
                    pointer_ms,
                    &model.dropped_source,
                ) {
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

    // トラック行（timeline.track.<id>）。
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

    // クリップ（timeline.clip.<id>）＋波形の概形。
    for rect in timeline::layout_clips(&model.project, win.w()) {
        let (r, g, b) = timeline::clip_color(rect.kind);
        let inner_y = rect.y + 4.0;
        let inner_h = rect.h - 8.0;
        let (cx, cy) = rect_center(win, rect.x, inner_y, rect.w, inner_h);
        draw.rect()
            .x_y(cx, cy)
            .w_h(rect.w, inner_h)
            .color(rgb(r, g, b));
        if let Some((_, clip)) = model.project.clip(rect.clip_id) {
            // 波形: クリップの表示範囲に対応するピークを縦線で描く。
            if let (Some(peaks), Some(&src_ms)) = (
                model.waveforms.get(&clip.source),
                model.source_durations.get(&clip.source),
            ) {
                if src_ms > 0 && rect.w >= 1.0 {
                    let mid_y = win.h() / 2.0 - (inner_y + inner_h / 2.0);
                    let cols = rect.w as usize;
                    for col in 0..cols {
                        let offset_ms = (col as f32 / rect.w) * clip.duration_ms as f32;
                        let frac = offset_ms / src_ms as f32;
                        let idx = (frac.clamp(0.0, 1.0) * (peaks.len() - 1) as f32) as usize;
                        let half = (peaks[idx] * inner_h / 2.0).max(0.5);
                        let nx = rect.x + col as f32 + 0.5 - win.w() / 2.0;
                        draw.line()
                            .start(pt2(nx, mid_y - half))
                            .end(pt2(nx, mid_y + half))
                            .weight(1.0)
                            .color(rgb(0.06, 0.10, 0.06));
                    }
                }
            }
            draw.text(&clip.name)
                .x_y(cx, cy)
                .font_size(11)
                .color(rgb(0.05, 0.05, 0.08));
        }
    }

    // ミキサー（mixer）: トラックごとの音量・パンスライダー（Issue #15）。
    for (index, track) in model.project.tracks.iter().enumerate() {
        let count = model.project.tracks.len();
        let v = mixer::volume_rect(count, index);
        let p = mixer::pan_rect(count, index);
        let top = mixer::mixer_top_y(count) + index as f32 * mixer::STRIP_HEIGHT_PX;
        let (sx, sy) = rect_center(win, 0.0, top, win.w(), mixer::STRIP_HEIGHT_PX);
        draw.rect()
            .x_y(sx, sy)
            .w_h(win.w(), mixer::STRIP_HEIGHT_PX)
            .color(rgb(0.10, 0.10, 0.12));
        // ラベルと現在値
        draw.text(&format!("{} VOL {:.2}", track.name, track.volume))
            .x_y(
                timeline::HEADER_WIDTH_PX / 2.0 - win.w() / 2.0,
                win.h() / 2.0 - (top + mixer::STRIP_HEIGHT_PX / 2.0),
            )
            .font_size(11)
            .color(rgb(0.80, 0.80, 0.85));
        for (rect, value_x, label) in [
            (v, mixer::x_for_volume(v, track.volume), "VOL"),
            (p, mixer::x_for_pan(p, track.pan), "PAN"),
        ] {
            let _ = label;
            let (bx, by) = rect_center(win, rect.x, rect.y, rect.w, rect.h);
            draw.rect()
                .x_y(bx, by)
                .w_h(rect.w, 4.0)
                .color(rgb(0.30, 0.30, 0.35));
            // つまみ
            let (kx, ky) = rect_center(win, value_x - 3.0, rect.y, 6.0, rect.h);
            draw.rect()
                .x_y(kx, ky)
                .w_h(6.0, rect.h)
                .color(rgb(0.90, 0.75, 0.25));
        }
        draw.text(&format!("PAN {:+.2}", track.pan))
            .x_y(
                mixer::PAN_X - 40.0 - win.w() / 2.0,
                win.h() / 2.0 - (top + mixer::STRIP_HEIGHT_PX / 2.0),
            )
            .font_size(11)
            .color(rgb(0.80, 0.80, 0.85));
    }

    // ピッチ表示（Issue #18）: 再生ヘッド位置の音声クリップのピッチ。
    let pitch_label = model.project.tracks.iter().find_map(|t| {
        if t.kind != studio_core::TrackKind::Audio {
            return None;
        }
        t.clips
            .iter()
            .find(|c| model.playhead_ms >= c.start_ms && model.playhead_ms < c.end_ms())
            .map(|c| format!("pitch: {} {:+.1} st", c.name, c.pitch_semitones))
    });
    if let Some(label) = pitch_label {
        draw.text(&label)
            .x_y(-win.w() / 2.0 + 130.0, -win.h() / 2.0 + 16.0)
            .font_size(12)
            .color(rgb(0.95, 0.75, 0.35));
    }

    // サンプラーの鍵盤（Issue #19）: 13 鍵を下部に描き、発音中を灯す。
    {
        let active: Vec<i32> = model
            .sampler
            .as_ref()
            .map(|s| s.active_notes())
            .unwrap_or_default();
        let key_w = 30.0f32;
        let key_h = 34.0f32;
        let x0 = -win.w() / 2.0 + 24.0;
        let y = -win.h() / 2.0 + 52.0;
        for i in 0..13i32 {
            let cx = x0 + (i as f32 + 0.5) * key_w;
            let is_active = active.contains(&i);
            let (r, g, b) = if is_active {
                (0.95, 0.75, 0.35)
            } else if [1, 3, 6, 8, 10].contains(&i) {
                (0.16, 0.16, 0.20)
            } else {
                (0.55, 0.55, 0.60)
            };
            draw.rect()
                .x_y(cx, y)
                .w_h(key_w - 3.0, key_h)
                .color(rgb(r, g, b));
        }
        let sampler_label = match &model.sampler {
            Some(s) => format!(
                "sampler: {} notes: {:?}",
                s.source_name.rsplit('/').next().unwrap_or(&s.source_name),
                active
            ),
            None => "sampler: (play keys A-K)".to_string(),
        };
        draw.text(&sampler_label)
            .x_y(x0 + 13.0 * key_w / 2.0, y + key_h / 2.0 + 12.0)
            .font_size(11)
            .color(rgb(0.80, 0.80, 0.85));
    }

    // 再生ヘッド（timeline.playhead）。
    let (pr, pg, pb) = timeline::PLAYHEAD_COLOR;
    let px = timeline::playhead_x(model.playhead_ms, win.w()) - win.w() / 2.0;
    draw.line()
        .start(pt2(px, win.h() / 2.0))
        .end(pt2(px, -win.h() / 2.0))
        .weight(2.0)
        .color(rgb(pr, pg, pb));

    draw.to_frame(app, &frame).unwrap();
}

/// Issue #13 のコンソールデモ（エンジンの動作確認用に残す）。
fn engine_demo() {
    let (tone220, tone440, _) = prepare_demo_media();
    let mut project = Project::new("daw demo session");
    let track = apply(
        &mut project,
        &Command::AddTrack {
            name: "A1".into(),
            kind: TrackKind::Audio,
        },
    )
    .unwrap()
    .unwrap();
    let bass = apply(
        &mut project,
        &Command::AddClip {
            track_id: track,
            name: "tone-220".into(),
            start_ms: 0,
            duration_ms: 2000,
            source: tone220.clone(),
        },
    )
    .unwrap()
    .unwrap();
    apply(
        &mut project,
        &Command::AddClip {
            track_id: track,
            name: "tone-440".into(),
            start_ms: 1000,
            duration_ms: 2000,
            source: tone440.clone(),
        },
    )
    .unwrap();
    apply(
        &mut project,
        &Command::SetClipGain {
            clip_id: bass,
            gain: 0.5,
        },
    )
    .unwrap();
    println!("daw (issue #13: audio engine)");
    println!("{}", project.dump_json());

    let mut audio_map = HashMap::new();
    for track in &project.tracks {
        for clip in &track.clips {
            let decoded = audio::decode_audio(std::path::Path::new(&clip.source))
                .expect("demo source must decode");
            println!(
                "decoded {}: {} Hz, {} ch, {} ms",
                clip.source,
                decoded.sample_rate,
                decoded.channels,
                decoded.duration_ms()
            );
            audio_map.entry(clip.source.clone()).or_insert(decoded);
        }
    }
    let end_ms = 3000;
    let mix = audio::render_mix(&project, &audio_map, 0, end_ms, RATE);
    let mix_path = media_dir().join("mixdown.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&mix_path, spec).unwrap();
    for s in &mix {
        writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
    }
    writer.finalize().unwrap();
    println!("mixdown written: {}", mix_path.display());
    for sec in 0..3 {
        let a = (sec * RATE as usize) * 2;
        let b = ((sec + 1) * RATE as usize) * 2;
        let rms = (mix[a..b].iter().map(|s| s * s).sum::<f32>() / (b - a) as f32).sqrt();
        println!("mix rms {sec}s-{}s: {rms:.4}", sec + 1);
    }
    let mut player = audio::Player::new(end_ms);
    player.play();
    for _ in 0..3 {
        player.advance(500);
        println!(
            "player: position={}ms playing={}",
            player.position_ms, player.playing
        );
    }
    player.pause();
    player.advance(500);
    println!(
        "player: paused, advance ignored: position={}ms playing={}",
        player.position_ms, player.playing
    );
    player.seek(0);
    player.play();
    for _ in 0..6 {
        player.advance(500);
        println!(
            "player: position={}ms playing={}",
            player.position_ms, player.playing
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_file_stamp_matches_dump_stamp() {
        // GUI が保存した直後に監視が再読込しないこと（#11 と同じ不変条件）。
        let project = demo_project("demo/a.wav", "demo/b.wav");
        let path =
            std::env::temp_dir().join(format!("rust-studio-daw-test-{}.json", std::process::id()));
        project.save_to_path(&path).unwrap();
        let file_bytes = std::fs::read(&path).unwrap();
        assert_eq!(fnv1a(&file_bytes), fnv1a(project.dump_json().as_bytes()));
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod watch_tests {
    use super::*;
    use studio_core::TrackKind;

    fn project_with_sources(sources: &[&str]) -> Project {
        let mut p = Project::new("watch test");
        let track = apply(
            &mut p,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        for (i, source) in sources.iter().enumerate() {
            apply(
                &mut p,
                &Command::AddClip {
                    track_id: track,
                    name: format!("clip-{i}"),
                    start_ms: i as i64 * 1000,
                    duration_ms: 500,
                    source: source.to_string(),
                },
            )
            .unwrap();
        }
        p
    }

    #[test]
    fn project_sources_lists_only_clip_sources_sorted() {
        // Issue #16 レビュー指摘: 再読込時のソース比較は、波形マップの
        // キー（配置用素材を含む）ではなくプロジェクトのクリップ
        // ソース同士で行う。
        let p = project_with_sources(&["b.wav", "a.wav", "b.wav"]);
        assert_eq!(project_sources(&p), vec!["a.wav", "b.wav"]);
    }

    #[test]
    fn sources_changed_detects_only_source_set_changes() {
        let a = project_with_sources(&["a.wav"]);
        let mut b = project_with_sources(&["a.wav"]);
        // 配置が変わっただけでは変わらない
        let clip_id = b.tracks[0].clips[0].id;
        apply(
            &mut b,
            &Command::MoveClip {
                clip_id,
                new_start_ms: 300,
            },
        )
        .unwrap();
        assert!(!sources_changed(&a, &b));
        // ソースが増えたら変わる
        let c = project_with_sources(&["a.wav", "c.wav"]);
        assert!(sources_changed(&a, &c));
    }

    #[test]
    fn refresh_waveforms_keeps_the_dropped_source() {
        // 再計算しても配置用素材の波形が消えないこと。
        let dir = std::env::temp_dir().join(format!("daw-watch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tone = dir.join("tone.wav");
        let dropped = dir.join("dropped.wav");
        write_tone(&tone, 440.0, 200);
        write_tone(&dropped, 330.0, 200);
        let tone_s = tone.to_string_lossy().to_string();
        let dropped_s = dropped.to_string_lossy().to_string();
        let p = project_with_sources(&[&tone_s]);
        let mut waveforms = HashMap::new();
        let mut durations = HashMap::new();
        refresh_waveforms(&p, &dropped_s, &mut waveforms, &mut durations);
        assert!(waveforms.contains_key(&tone_s), "clip source waveform");
        assert!(
            waveforms.contains_key(&dropped_s),
            "dropped source waveform must be kept"
        );
        assert!(durations.contains_key(&dropped_s));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
