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
mod palette;
mod sampler;
// theme は配色表の全色を先に定義する土台（Issue #23）。アクセント系と
// トランスポート系の定数は後続 PBI（#24〜#27）で使うため、未使用の
// 警告をモジュール単位で抑止する。
#[allow(dead_code)]
mod theme;
mod timeline;

use std::collections::HashMap;

use nannou::prelude::*;
use studio_core::{apply, ColorTag, Command, History, Project, TrackKind};

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
    history: History,
    playhead_ms: i64,
    drag: Option<edit::Drag>,
    last_drag_cmd: Option<Command>,
    // Issue #31: ジェスチャ（ドラッグ／ミキサー操作）開始時の
    // スナップショット。履歴への記録はジェスチャ終了時に行う。
    drag_before: Option<Project>,
    mixer_before: Option<Project>,
    last_rejection: Option<String>,
    prev_left_down: bool,
    clicks: edit::ClickTracker,
    /// トラック操作（Issue #56）: ヘッダーのダブルクリック判定と
    /// リネーム編集セッション。どちらも GUI の表示状態で、
    /// プロジェクト状態には含めない（#25 の選択状態と同じ扱い）。
    header_clicks: edit::ClickTracker,
    rename: Option<edit::RenameSession>,
    /// ソースごとの波形ピーク（全体を WAVEFORM_COLUMNS 本で事前計算）。
    waveforms: HashMap<String, Vec<f32>>,
    /// ソースごとの長さ（ms）。波形の切り出しに使う。
    source_durations: HashMap<String, i64>,
    dropped_source: String,
    /// 操作中のミキサースライダー（トラック ID とパラメータ）。
    mixer_active: Option<(u64, mixer::Param)>,
    /// 選択中トラック（GUI の表示状態。プロジェクト状態ではない、
    /// Issue #25）。現在値を大きく出す対象。
    selected_track: Option<u64>,
    /// 最後にノートオンした半音数（GUI の表示状態、Issue #26）。
    /// 現在のピッチの大きな表示に使う。
    sampler_last: Option<i32>,
    /// デコード済み音声（ソース→音声、Issue #27）。レベルメーターが
    /// ミックスをレンダリングしてピークを取るために保持する。
    audio_map: HashMap<String, audio::ClipAudio>,
    /// 直近に計算したミックスのピーク（線形、Issue #27）。
    level_peak: f32,
    /// ピークを計算したときの再生位置（ms）。再計算の間引きに使う。
    level_at_ms: i64,
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
    // Issue #31: Undo/Redo キーの押下エッジ追跡。
    history_keys: edit::HistoryKeyTracker,
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

/// プロジェクトのソース集合をデコードした音声マップ（Issue #27）。
/// レベルメーターが `render_mix` でピークを取るために保持する
/// （読み取り専用。波形と同じくソース単位でデコードする）。
fn compute_audio_map(project: &Project, dropped_source: &str) -> HashMap<String, audio::ClipAudio> {
    let mut map = HashMap::new();
    let mut sources: Vec<&str> = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.source.as_str()))
        .collect();
    sources.push(dropped_source);
    sources.sort();
    sources.dedup();
    for source in sources {
        if let Ok(decoded) = audio::decode_audio(std::path::Path::new(source)) {
            map.insert(source.to_string(), decoded);
        }
    }
    map
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
    let audio_map = compute_audio_map(&project, &dropped);
    let window = app
        .new_window()
        .size(960, 540)
        .title("daw")
        .view(view)
        .build()
        .unwrap();
    Model {
        _window: window,
        selected_track: project.tracks.first().map(|t| t.id),
        history: History::new(project),
        playhead_ms: 0,
        drag: None,
        last_drag_cmd: None,
        drag_before: None,
        mixer_before: None,
        last_rejection: None,
        prev_left_down: false,
        clicks: edit::ClickTracker::new(),
        header_clicks: edit::ClickTracker::new(),
        rename: None,
        waveforms,
        source_durations,
        dropped_source: dropped,
        mixer_active: None,
        sampler_last: None,
        audio_map,
        level_peak: 0.0,
        level_at_ms: -1_000,
        last_mixer_cmd: None,
        project_file,
        file_stamp,
        last_watch_check: 0.0,
        prev_pitch_up: false,
        prev_pitch_down: false,
        sampler: None,
        prev_note_keys: [false; 13],
        history_keys: edit::HistoryKeyTracker::new(),
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
    nannou::app(model).event(event).update(update).run();
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
            model.drag_before = None;
            model.mixer_before = None;
            // リネーム編集も打ち切る（バグ #60: 対象が消えると Enter の
            // たびに拒否され続け、残っていても外部変更後の名前を編集中
            // テキストで黙って上書きしてしまうため）。
            model.rename = None;
            // ソース集合の比較はプロジェクトのクリップ同士で行い、
            // 再計算では配置用素材の波形を保持する（Issue #16 レビュー指摘）。
            let changed = sources_changed(model.history.project(), &new_project);
            model.history = History::new(new_project);
            if changed {
                refresh_waveforms(
                    model.history.project(),
                    &model.dropped_source,
                    &mut model.waveforms,
                    &mut model.source_durations,
                );
                model.audio_map = compute_audio_map(model.history.project(), &model.dropped_source);
            }
            println!("project reloaded: {}", model.history.project().dump_json());
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
    match model.history.project().save_to_path(&path) {
        Ok(()) => {
            model.file_stamp = fnv1a(model.history.project().dump_json().as_bytes());
        }
        Err(e) => println!("warning: cannot save project file {}: {e}", path.display()),
    }
}

/// キーボード入力（Issue #56）: リネーム編集中だけ受け付ける。
/// Enter で RenameTrack コマンド（#55）を発行し、Esc で中止する。
/// 確定がコマンド層に弾かれた場合（空白名など）は編集を続け、
/// 理由をログに出す（イシューの実装方針）。
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
        edit::RenameAction::None => {}
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
    // リネーム編集中は他の操作（ポインタ編集・ピッチ・サンプラー）を
    // 受け付けない（Issue #56）。再生ヘッドとファイル監視は続ける。
    let renaming = model.rename.is_some();
    if renaming {
        model.prev_left_down = app.mouse.buttons.left().is_down();
        // エッジ検出用の prev は編集中も現在値に同期する（バグ #60:
        // 同期しないと、編集中に押したキーを押したまま編集を終えた
        // 次のフレームで「新規押下」と誤判定され、意図しないピッチ
        // 変更やサンプラー発音が起きた）。ハンドラ本体は呼ばない。
        model.prev_pitch_up = app.keys.down.contains(&Key::Up);
        model.prev_pitch_down = app.keys.down.contains(&Key::Down);
        for (idx, (key, _)) in NOTE_KEYS.iter().enumerate() {
            model.prev_note_keys[idx] = app.keys.down.contains(key);
        }
        model.history_keys.sync(
            app.keys.down.contains(&Key::Z),
            app.keys.down.contains(&Key::Y),
        );
    } else {
        handle_pointer(app, model);
    }
    watch_project_file(app, model);
    if !renaming {
        handle_history_keys(app, model);
        handle_pitch_keys(app, model);
    }
    let dt_ms = (update.since_last.as_secs_f64() * 1000.0) as i64;
    if !renaming {
        if ctrl_down(app) {
            // Ctrl 併用時はサンプラーを鳴らさない（Ctrl+Y の redo と
            // ノートキーの Y が重なるため）。prev は同期して、離した
            // 後の誤発を防ぐ。
            for (idx, (key, _)) in NOTE_KEYS.iter().enumerate() {
                model.prev_note_keys[idx] = app.keys.down.contains(key);
            }
        } else {
            handle_sampler_keys(app, model);
        }
    }
    if let Some(sampler) = &mut model.sampler {
        sampler.advance(dt_ms);
    }
    let end = timeline::project_end_ms(model.history.project());
    model.playhead_ms = timeline::advance_playhead(model.playhead_ms, dt_ms, end);
    // レベルメーター（Issue #27）: 再生位置の直後のミックスを
    // レンダリングしてピークを取る（読み取り専用。音声処理には
    // 手を入れない）。再生位置が 50ms 以上動いたときだけ再計算する。
    if (model.playhead_ms - model.level_at_ms).abs() >= 50 {
        let mix = audio::render_mix(
            model.history.project(),
            &model.audio_map,
            model.playhead_ms,
            LEVEL_WINDOW_MS,
            44_100,
        );
        model.level_peak = audio::peak_of(&mix);
        model.level_at_ms = model.playhead_ms;
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

/// Undo/Redo キー（Issue #31）: Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y。
/// 履歴が実際に動いたときだけ、派生状態（波形・音声マップ・選択・
/// ドラッグ）を追従させ、プロジェクトファイルへ保存する。
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
    // 進行中ジェスチャ（ドラッグ／ミキサー）があれば、履歴操作の前に
    // 確定させる。記録をジェスチャ終了時まで遅延しているため、この
    // まま undo すると未確定の途中状態ではなく直前の確定編集が消費
    // され、確定状態が undo/redo どちらのスタックにも残らなくなる
    // （#31 レビュー round 3 対応）。先に確定させれば、undo は
    // ジェスチャ自身のエントリを消費して開始状態へ戻る。
    finish_gesture_snapshot(&mut model.history, &mut model.drag_before);
    finish_gesture_snapshot(&mut model.history, &mut model.mixer_before);
    if !edit::apply_history_op(&mut model.history, op) {
        return;
    }
    model.drag = None;
    model.last_drag_cmd = None;
    model.last_rejection = None;
    model.drag_before = None;
    model.mixer_active = None;
    model.last_mixer_cmd = None;
    model.mixer_before = None;
    if model
        .selected_track
        .is_some_and(|id| model.history.project().track(id).is_none())
    {
        model.selected_track = model.history.project().tracks.first().map(|t| t.id);
    }
    refresh_waveforms(
        model.history.project(),
        &model.dropped_source,
        &mut model.waveforms,
        &mut model.source_durations,
    );
    model.audio_map = compute_audio_map(model.history.project(), &model.dropped_source);
    let label = match op {
        edit::HistoryOp::Undo => "undo",
        edit::HistoryOp::Redo => "redo",
    };
    println!("after {label}: {}", model.history.project().dump_json());
    save_project_file(model);
}

/// レベルメーターがピークを取るミックスの窓（ms、Issue #27）。
const LEVEL_WINDOW_MS: i64 = 100;

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
                    .history
                    .project()
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
                model.sampler_last = Some(semitones);
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
    let Some((_, clip)) = model.history.project().tracks.iter().find_map(|t| {
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
    match model.history.apply(&cmd) {
        Ok(_) => {
            println!("after pitch: {}", model.history.project().dump_json());
            save_project_file(model);
        }
        Err(e) => println!("command rejected: {e}"),
    }
}

/// 色選択パネルのクリックをコマンドに変換する（Issue #58）。
/// TRACK 行は選択中トラック、CLIP 行は選択中トラックの再生位置にある
/// クリップ（ピッチキーと同じ対象）に適用する。状態変更はコマンド
/// 経由のみ（ADR-0004）。
fn apply_palette_click(model: &mut Model, target: palette::PaletteTarget, color: Option<ColorTag>) {
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
/// 終了時まで遅らせることで、最終状態と開始状態が同じなら
/// `History::record_undo_snapshot` の同値ガードが記録自体を省く:
/// 動かさないクリックはもちろん、一度動かして原位置へ戻した
/// ドラッグでも「幽霊 undo」を積んだり redo を消したりしない
/// （#31 レビュー round 2 対応）。
fn finish_gesture_snapshot(history: &mut History, before: &mut Option<Project>) {
    if let Some(snapshot) = before.take() {
        history.record_undo_snapshot(snapshot);
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
        if let Some(hit) = mixer::hit_mixer(model.history.project(), lx, ly) {
            model.mixer_active = Some((hit.track_id, hit.param));
            model.mixer_before = Some(model.history.project().clone());
            let cmd = mixer::command_for(&hit);
            if model.history.apply_transient(&cmd).is_ok() {
                model.last_mixer_cmd = Some(cmd);
            }
        }
    }
    if left_down {
        if let Some((track_id, param)) = model.mixer_active {
            if let Some(hit) = mixer::hit_with_x(model.history.project(), track_id, param, lx) {
                let cmd = mixer::command_for(&hit);
                if model.last_mixer_cmd.as_ref() != Some(&cmd) {
                    match model.history.apply_transient(&cmd) {
                        Ok(_) => {
                            model.last_mixer_cmd = Some(cmd);
                        }
                        Err(e) => println!("command rejected: {e}"),
                    }
                }
            }
        }
    }
    if !left_down && model.prev_left_down && model.mixer_active.take().is_some() {
        // ジェスチャ終了時にだけ履歴へ記録する（Issue #31）。
        finish_gesture_snapshot(&mut model.history, &mut model.mixer_before);
        println!("after mixer: {}", model.history.project().dump_json());
        save_project_file(model);
        model.last_mixer_cmd = None;
    }

    if left_down && !model.prev_left_down {
        // 色選択パネル（Issue #58）: トランスポートの下のパネル。
        // 他の領域とは重ならない。
        if let Some((target, color)) =
            palette::hit_palette(model.history.project().tracks.len(), lx, ly)
        {
            apply_palette_click(model, target, color);
            model.prev_left_down = left_down;
            return;
        }
        // トラック削除（Issue #56）: ヘッダー右端の帯。コマンド経由で
        // 削除し、選択が消えたら同じ位置（なければ最後）のトラックへ
        // 選択を移す。削除したトラックの操作状態も打ち切る。
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
                    if model.mixer_active.map(|(id, _)| id) == Some(track_id) {
                        model.mixer_active = None;
                        model.last_mixer_cmd = None;
                        model.mixer_before = None;
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
        // トラック追加（Issue #56）: 最終行直下のヘッダー列。
        // 追加したトラックを選択する。
        if edit::hit_track_add_area(model.history.project(), lx, ly) {
            let cmd = edit::add_track_command(model.history.project());
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
        // トラックヘッダーのクリックで選択（Issue #25、表示状態のみ）。
        // ダブルクリックでリネーム編集に入る（Issue #56）。
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
            model.last_rejection = None;
            model.drag_before = Some(model.history.project().clone());
        } else if let edit::HitTarget::Empty { track_index } = target {
            let now_ms = (app.time * 1_000.0) as u64;
            if model.clicks.register(now_ms, lx, ly) {
                if let Some(cmd) = edit::place_clip_command(
                    model.history.project(),
                    track_index,
                    pointer_ms,
                    &model.dropped_source,
                ) {
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
            let cmd = drag.command_for(pointer_ms);
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

/// パネルラベル（連番＋大文字の小ラベル）を、左上原点ピクセル座標の
/// 点を中心に描く（デザイン文書「タイポグラフィとパネルラベル」、
/// Issue #23）。色はグレージュ、サイズは theme の定数。
fn draw_panel_label(draw: &Draw, win: Rect, label: &str, cx: f32, cy: f32) {
    let (r, g, b) = theme::TEXT_SUB;
    draw.text(label)
        .x_y(cx - win.w() / 2.0, win.h() / 2.0 - cy)
        .font_size(theme::LABEL_FONT_SIZE)
        .color(rgb(r, g, b));
}

/// 1px の横線（区切り線）を左上原点ピクセル座標で描く。
fn draw_hline(draw: &Draw, win: Rect, y: f32, color: theme::Rgb) {
    let (cx, cy) = rect_center(win, 0.0, y, win.w(), 1.0);
    draw.rect()
        .x_y(cx, cy)
        .w_h(win.w(), 1.0)
        .color(rgb(color.0, color.1, color.2));
}

/// 角丸矩形を左上原点ピクセル座標で描く（Issue #24）。nannou の
/// `rect` には角丸がないため、2枚の矩形と四隅の円で近似する。
fn draw_rounded_rect(draw: &Draw, win: Rect, rect: mixer::Rect, radius: f32, color: theme::Rgb) {
    let (x, y, w, h) = (rect.x, rect.y, rect.w, rect.h);
    let r = radius.min(w / 2.0).min(h / 2.0);
    let c = rgb(color.0, color.1, color.2);
    let (cx, cy) = rect_center(win, x + r, y, w - 2.0 * r, h);
    draw.rect().x_y(cx, cy).w_h(w - 2.0 * r, h).color(c);
    let (cx, cy) = rect_center(win, x, y + r, w, h - 2.0 * r);
    draw.rect().x_y(cx, cy).w_h(w, h - 2.0 * r).color(c);
    for (ex, ey) in [
        (x + r, y + r),
        (x + w - r, y + r),
        (x + r, y + h - r),
        (x + w - r, y + h - r),
    ] {
        let (cx, cy) = rect_center(win, ex - r, ey - r, 2.0 * r, 2.0 * r);
        draw.ellipse().x_y(cx, cy).w_h(2.0 * r, 2.0 * r).color(c);
    }
}

/// ピッチ表示の文字色（Issue #28）。
///
/// ピッチ表示は小さい補助文字（キャプション）なので、デザイン文書の
/// 役割づけに従い `theme::TEXT_SUB`（グレージュ）に統一する。
/// 旧来の金色リテラルは #23〜#27 のテーマ適用から漏れていた残件。
fn pitch_label_color() -> theme::Rgb {
    theme::TEXT_SUB
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    let win = app.window_rect();
    let (bg_r, bg_g, bg_b) = theme::BACKGROUND;
    draw.background().color(rgb(bg_r, bg_g, bg_b));

    // ルーラー（timeline.ruler）: 帯＋1秒ごとの目盛りと秒ラベル。
    let (rx, ry) = rect_center(win, 0.0, 0.0, win.w(), timeline::RULER_HEIGHT_PX);
    let (r, g, b) = theme::PANEL_SUNK;
    draw.rect()
        .x_y(rx, ry)
        .w_h(win.w(), timeline::RULER_HEIGHT_PX)
        .color(rgb(r, g, b));
    draw_hline(&draw, win, timeline::RULER_HEIGHT_PX - 1.0, theme::LINE);
    draw_panel_label(
        &draw,
        win,
        theme::TIMELINE_LABEL,
        44.0,
        timeline::RULER_HEIGHT_PX / 2.0,
    );
    for sec in 0..=(timeline::VIEW_DURATION_MS / 1_000) {
        let x = timeline::time_to_x(sec * 1_000, win.w());
        let nx = x - win.w() / 2.0;
        let (r, g, b) = theme::LINE;
        draw.line()
            .start(pt2(nx, win.h() / 2.0 - timeline::RULER_HEIGHT_PX))
            .end(pt2(nx, win.h() / 2.0 - timeline::RULER_HEIGHT_PX + 6.0))
            .weight(1.0)
            .color(rgb(r, g, b));
        let (r, g, b) = theme::TEXT_SUB;
        draw.text(&format!("{sec}s"))
            .x_y(nx + 10.0, win.h() / 2.0 - timeline::RULER_HEIGHT_PX / 2.0)
            .font_size(10)
            .color(rgb(r, g, b));
    }

    // トラック行（timeline.track.<id>）。
    for (index, track) in model.history.project().tracks.iter().enumerate() {
        let top = timeline::track_top_y(index);
        let (bx, by) = rect_center(win, 0.0, top, win.w(), timeline::TRACK_HEIGHT_PX);
        let (r, g, b) = theme::PANEL;
        draw.rect()
            .x_y(bx, by)
            .w_h(win.w(), timeline::TRACK_HEIGHT_PX)
            .color(rgb(r, g, b));
        draw_hline(
            &draw,
            win,
            top + timeline::TRACK_HEIGHT_PX - 1.0,
            theme::LINE,
        );
        let (hx, hy) = rect_center(
            win,
            0.0,
            top,
            timeline::HEADER_WIDTH_PX,
            timeline::TRACK_HEIGHT_PX,
        );
        let (r, g, b) = theme::PANEL_SUNK;
        draw.rect()
            .x_y(hx, hy)
            .w_h(timeline::HEADER_WIDTH_PX, timeline::TRACK_HEIGHT_PX)
            .color(rgb(r, g, b));
        // トラック色（Issue #58）: ヘッダー左端に色の帯を出す。
        if let Some(tag) = track.color {
            let (face, _) = theme::palette_colors(tag);
            let (bar_x, bar_y) = rect_center(win, 0.0, top, 6.0, timeline::TRACK_HEIGHT_PX);
            draw.rect()
                .x_y(bar_x, bar_y)
                .w_h(6.0, timeline::TRACK_HEIGHT_PX)
                .color(rgb(face.0, face.1, face.2));
        }
        let (r, g, b) = theme::TEXT;
        // リネーム編集中のトラックは編集中の文字列＋キャレットを出す
        // （Issue #56）。それ以外はトラック名。名前は削除帯を除いた
        // 範囲の中央に置き、「×」と重ならないようにする。
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
            .color(rgb(r, g, b));
        // 削除ボタン（Issue #56）: ヘッダー右端の帯に「×」。
        let (xx, xy) = rect_center(
            win,
            timeline::HEADER_WIDTH_PX - edit::DELETE_STRIP_PX,
            top,
            edit::DELETE_STRIP_PX,
            timeline::TRACK_HEIGHT_PX,
        );
        let (r, g, b) = theme::TEXT_SUB;
        draw.text("×").x_y(xx, xy).font_size(12).color(rgb(r, g, b));
    }

    // トラック追加エリア（Issue #56）: 最終行直下のヘッダー列。
    {
        let top = timeline::track_top_y(model.history.project().tracks.len());
        let (ax, ay) = rect_center(
            win,
            0.0,
            top,
            timeline::HEADER_WIDTH_PX,
            timeline::TRACK_HEIGHT_PX,
        );
        let (r, g, b) = theme::PANEL_SUNK;
        draw.rect()
            .x_y(ax, ay)
            .w_h(timeline::HEADER_WIDTH_PX, timeline::TRACK_HEIGHT_PX)
            .color(rgb(r, g, b));
        let (r, g, b) = theme::TEXT_SUB;
        draw.text("+ TRACK")
            .x_y(ax, ay)
            .font_size(11)
            .color(rgb(r, g, b));
    }

    // クリップ（timeline.clip.<id>）＋波形の概形。
    for rect in timeline::layout_clips(model.history.project(), win.w()) {
        let clip = model.history.project().clip(rect.clip_id).map(|(_, c)| c);
        let playing = clip
            .map(|c| timeline::clip_is_playing(c, model.playhead_ms))
            .unwrap_or(false);
        let (face, edge) =
            timeline::clip_colors_for(clip.and_then(|c| c.color), rect.kind, playing);
        let inner_y = rect.y + 4.0;
        let inner_h = rect.h - 8.0;
        let (cx, cy) = rect_center(win, rect.x, inner_y, rect.w, inner_h);
        // 縁→面の順に角丸で描く（Issue #24、デザイン文書の対応表）。
        draw_rounded_rect(
            &draw,
            win,
            mixer::Rect {
                x: rect.x,
                y: inner_y,
                w: rect.w,
                h: inner_h,
            },
            theme::CHIP_CORNER_RADIUS,
            edge,
        );
        draw_rounded_rect(
            &draw,
            win,
            mixer::Rect {
                x: rect.x + 1.5,
                y: inner_y + 1.5,
                w: rect.w - 3.0,
                h: inner_h - 3.0,
            },
            theme::CHIP_CORNER_RADIUS,
            face,
        );
        if let Some(clip) = clip {
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
                            .color(rgb(theme::TEAL.0, theme::TEAL.1, theme::TEAL.2));
                    }
                }
            }
            draw.text(&clip.name).x_y(cx, cy).font_size(11).color(rgb(
                theme::TEXT.0,
                theme::TEXT.1,
                theme::TEXT.2,
            ));
        }
    }

    // ミキサー（mixer）: トラックごとの音量・パンスライダー（Issue #15）。
    // 選択中トラック（無効な ID なら先頭トラックにフォールバック）。
    let selected_id = model
        .selected_track
        .filter(|id| model.history.project().tracks.iter().any(|t| t.id == *id))
        .or_else(|| model.history.project().tracks.first().map(|t| t.id));
    for (index, track) in model.history.project().tracks.iter().enumerate() {
        let count = model.history.project().tracks.len();
        let v = mixer::volume_rect(count, index);
        let p = mixer::pan_rect(count, index);
        let top = mixer::mixer_top_y(count) + index as f32 * mixer::STRIP_HEIGHT_PX;
        let (sx, sy) = rect_center(win, 0.0, top, win.w(), mixer::STRIP_HEIGHT_PX);
        let (r, g, b) = theme::PANEL;
        draw.rect()
            .x_y(sx, sy)
            .w_h(win.w(), mixer::STRIP_HEIGHT_PX)
            .color(rgb(r, g, b));
        draw_hline(&draw, win, top, theme::LINE);
        if index == 0 {
            draw_panel_label(&draw, win, theme::MIXER_LABEL, 40.0, top + 7.0);
        }
        // トラック色（Issue #58）: ストリップ左端にも色の帯を出す
        // （タイムラインのヘッダーと同じ表現）。
        if let Some(tag) = track.color {
            let (face, _) = theme::palette_colors(tag);
            let (bar_x, bar_y) = rect_center(win, 0.0, top, 6.0, mixer::STRIP_HEIGHT_PX);
            draw.rect()
                .x_y(bar_x, bar_y)
                .w_h(6.0, mixer::STRIP_HEIGHT_PX)
                .color(rgb(face.0, face.1, face.2));
        }
        // ラベルと現在値（補助文字はグレージュ、Issue #25）
        draw.text(&format!("{} VOL {:.2}", track.name, track.volume))
            .x_y(
                timeline::HEADER_WIDTH_PX / 2.0 - win.w() / 2.0,
                win.h() / 2.0 - (top + mixer::STRIP_HEIGHT_PX / 2.0),
            )
            .font_size(11)
            .color(rgb(theme::TEXT_SUB.0, theme::TEXT_SUB.1, theme::TEXT_SUB.2));
        for (rect, value_x, param) in [
            (
                v,
                mixer::x_for_volume(v, track.volume),
                mixer::Param::Volume,
            ),
            (p, mixer::x_for_pan(p, track.pan), mixer::Param::Pan),
        ] {
            // スライダーの溝（バー背景はサンド）
            let (bx, by) = rect_center(win, rect.x, rect.y, rect.w, rect.h);
            draw.rect().x_y(bx, by).w_h(rect.w, 4.0).color(rgb(
                theme::PANEL_SUNK.0,
                theme::PANEL_SUNK.1,
                theme::PANEL_SUNK.2,
            ));
            // つまみ: 通常はペールクリーム面＋グレージュ線の縁、
            // ドラッグ中だけホットピンク（デザイン文書の強調原則）。
            let dragging = model.mixer_active == Some((track.id, param));
            let knob_face = if dragging {
                theme::ACCENT
            } else {
                theme::PANEL
            };
            draw_rounded_rect(
                &draw,
                win,
                mixer::Rect {
                    x: value_x - 5.0,
                    y: rect.y,
                    w: 10.0,
                    h: rect.h,
                },
                3.0,
                theme::LINE,
            );
            draw_rounded_rect(
                &draw,
                win,
                mixer::Rect {
                    x: value_x - 3.5,
                    y: rect.y + 1.5,
                    w: 7.0,
                    h: rect.h - 3.0,
                },
                2.0,
                knob_face,
            );
        }
        draw.text(&format!("PAN {:+.2}", track.pan))
            .x_y(
                mixer::PAN_X - 40.0 - win.w() / 2.0,
                win.h() / 2.0 - (top + mixer::STRIP_HEIGHT_PX / 2.0),
            )
            .font_size(11)
            .color(rgb(theme::TEXT_SUB.0, theme::TEXT_SUB.1, theme::TEXT_SUB.2));
        // 選択中トラックの現在値は大きく出す（Issue #25、デザイン
        // 文書「タイポグラフィとパネルラベル」: 階層はサイズで作る）。
        if selected_id == Some(track.id) {
            let readout = format!(
                "{} · {}",
                mixer::format_volume_db(track.volume),
                mixer::format_pan(track.pan)
            );
            draw.text(&readout)
                .x_y(
                    872.0 - win.w() / 2.0,
                    win.h() / 2.0 - (top + mixer::STRIP_HEIGHT_PX / 2.0),
                )
                .font_size(theme::VALUE_FONT_SIZE)
                .color(rgb(theme::TEXT.0, theme::TEXT.1, theme::TEXT.2));
        }
    }

    // トランスポート（Issue #27）: 進行度（小節・拍の数値＋横バー）と
    // レベルメーターを常時表示する。01 TRANSPORT ラベルは、この
    // パネルを作る本イシューで描く（#23 では定数だけ用意した）。
    {
        let count = model.history.project().tracks.len();
        let top = mixer::mixer_top_y(count) + count as f32 * mixer::STRIP_HEIGHT_PX + 28.0;
        draw_rounded_rect(
            &draw,
            win,
            mixer::Rect {
                x: 8.0,
                y: top - 8.0,
                w: win.w() - 16.0,
                h: 64.0,
            },
            theme::PANEL_CORNER_RADIUS,
            theme::PANEL,
        );
        draw_panel_label(&draw, win, theme::TRANSPORT_LABEL, 40.0, top);
        let end_ms = timeline::project_end_ms(model.history.project()).max(1);
        let progress = (model.playhead_ms as f32 / end_ms as f32).clamp(0.0, 1.0);
        // 現在地の数値（小節・拍）は大きく出す
        draw.text(&format!(
            "BAR {} · BEAT {}",
            timeline::bar_at_ms(model.playhead_ms),
            timeline::beat_at_ms(model.playhead_ms)
        ))
        .x_y(115.0 - win.w() / 2.0, win.h() / 2.0 - (top + 27.0))
        .font_size(theme::VALUE_FONT_SIZE)
        .color(rgb(theme::TEXT.0, theme::TEXT.1, theme::TEXT.2));
        // 進行バー: 溝はサンド、進んだ分はピンク
        let bar = mixer::Rect {
            x: 230.0,
            y: top + 14.0,
            w: 460.0,
            h: 7.0,
        };
        draw_rounded_rect(&draw, win, bar, 3.0, theme::PANEL_SUNK);
        if progress > 0.0 {
            draw_rounded_rect(
                &draw,
                win,
                mixer::Rect {
                    w: bar.w * progress,
                    ..bar
                },
                3.0,
                theme::ACCENT,
            );
        }
        // レベルメーター: 溝はサンド、レベルはティール、ピーク付近
        // （-3dB 以上）だけホットピンク
        let db = audio::peak_db(model.level_peak);
        let frac = if db.is_finite() {
            ((db - audio::METER_FLOOR_DB) / -audio::METER_FLOOR_DB).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let meter = mixer::Rect {
            x: 230.0,
            y: top + 31.0,
            w: 460.0,
            h: 7.0,
        };
        draw_rounded_rect(&draw, win, meter, 3.0, theme::PANEL_SUNK);
        if frac > 0.0 {
            let color = if audio::is_peak_level(db) {
                theme::ACCENT
            } else {
                theme::TEAL
            };
            draw_rounded_rect(
                &draw,
                win,
                mixer::Rect {
                    w: meter.w * frac,
                    ..meter
                },
                3.0,
                color,
            );
        }
        let db_label = if db.is_finite() {
            format!("{db:.1} dB")
        } else {
            "-inf dB".to_string()
        };
        draw.text(&db_label)
            .x_y(745.0 - win.w() / 2.0, win.h() / 2.0 - (top + 34.0))
            .font_size(11)
            .color(rgb(theme::TEXT_SUB.0, theme::TEXT_SUB.1, theme::TEXT_SUB.2));
        draw.text(&format!(
            "{:.1} / {:.1} s",
            model.playhead_ms as f32 / 1000.0,
            end_ms as f32 / 1000.0
        ))
        .x_y(880.0 - win.w() / 2.0, win.h() / 2.0 - (top + 17.0))
        .font_size(11)
        .color(rgb(theme::TEXT_SUB.0, theme::TEXT_SUB.1, theme::TEXT_SUB.2));
    }

    // 色選択パネル（Issue #58）: トランスポートの下。TRACK 行は選択中
    // トラック、CLIP 行は選択中トラックの再生位置にあるクリップが対象。
    // 現在の色のスウォッチにはウォームブラックの枠を付ける。
    {
        let count = model.history.project().tracks.len();
        let top = palette::panel_top(count);
        draw_rounded_rect(
            &draw,
            win,
            mixer::Rect {
                x: 8.0,
                y: top - 8.0,
                w: win.w() - 16.0,
                h: palette::panel_height(),
            },
            theme::PANEL_CORNER_RADIUS,
            theme::PANEL,
        );
        draw_panel_label(&draw, win, theme::COLOR_LABEL, 40.0, top);
        let selected_track = model
            .selected_track
            .and_then(|id| model.history.project().track(id));
        let playhead_clip = selected_track.and_then(|t| {
            t.clips
                .iter()
                .find(|c| timeline::clip_is_playing(c, model.playhead_ms))
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
            let (lx, ly) = rect_center(
                win,
                8.0,
                row_y,
                palette::ROW_LABEL_W_PX + 8.0,
                palette::ROW_H_PX,
            );
            draw.text(label).x_y(lx, ly).font_size(10).color(rgb(
                theme::TEXT_SUB.0,
                theme::TEXT_SUB.1,
                theme::TEXT_SUB.2,
            ));
            for index in 0..=theme::PALETTE.len() {
                let rect = palette::swatch_rect(top, target, index);
                let is_current = match index {
                    0 => current.is_none(),
                    i => current == Some(theme::PALETTE[i - 1].0),
                };
                if is_current {
                    draw_rounded_rect(
                        &draw,
                        win,
                        mixer::Rect {
                            x: rect.x - 2.0,
                            y: rect.y - 2.0,
                            w: rect.w + 4.0,
                            h: rect.h + 4.0,
                        },
                        theme::CHIP_CORNER_RADIUS,
                        theme::TEXT,
                    );
                }
                if index == 0 {
                    // 「なし」: 淡い面＋×（未設定に戻す）
                    draw_rounded_rect(
                        &draw,
                        win,
                        rect,
                        theme::CHIP_CORNER_RADIUS,
                        theme::PANEL_SUNK,
                    );
                    let (cx, cy) = rect_center(win, rect.x, rect.y, rect.w, rect.h);
                    draw.text("×").x_y(cx, cy).font_size(11).color(rgb(
                        theme::TEXT_SUB.0,
                        theme::TEXT_SUB.1,
                        theme::TEXT_SUB.2,
                    ));
                } else {
                    let (_, face, _) = theme::PALETTE[index - 1];
                    draw_rounded_rect(&draw, win, rect, theme::CHIP_CORNER_RADIUS, face);
                }
            }
        }
    }

    // ピッチ表示（Issue #18）: 再生ヘッド位置の音声クリップのピッチ。
    let pitch_label = model.history.project().tracks.iter().find_map(|t| {
        if t.kind != studio_core::TrackKind::Audio {
            return None;
        }
        t.clips
            .iter()
            .find(|c| model.playhead_ms >= c.start_ms && model.playhead_ms < c.end_ms())
            .map(|c| format!("pitch: {} {:+.1} st", c.name, c.pitch_semitones))
    });
    if let Some(label) = pitch_label {
        let c = pitch_label_color();
        draw.text(&label)
            .x_y(-win.w() / 2.0 + 130.0, -win.h() / 2.0 + 16.0)
            .font_size(12)
            .color(rgb(c.0, c.1, c.2));
    }

    // サンプラーの鍵盤（Issue #19）: 13 鍵を下部に描き、発音中を灯す。
    {
        draw_panel_label(&draw, win, theme::SAMPLER_LABEL, 40.0, win.h() - 100.0);
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
            let is_active = active.contains(&i);
            // 鍵は角丸チップ（Issue #26）: 白鍵はペールクリーム面＋
            // グレージュの縁、黒鍵はウォームブラック、発音中だけ
            // ホットピンク（デザイン文書の強調原則）。
            let (edge, face) = if is_active {
                (theme::ACCENT, theme::ACCENT)
            } else if [1, 3, 6, 8, 10].contains(&i) {
                (theme::TEXT, theme::TEXT)
            } else {
                (theme::LINE, theme::PANEL)
            };
            let chip = mixer::Rect {
                x: 24.0 + i as f32 * key_w + 1.5,
                y: win.h() - 69.0,
                w: key_w - 3.0,
                h: key_h,
            };
            draw_rounded_rect(&draw, win, chip, theme::CHIP_CORNER_RADIUS, edge);
            draw_rounded_rect(
                &draw,
                win,
                mixer::Rect {
                    x: chip.x + 1.5,
                    y: chip.y + 1.5,
                    w: chip.w - 3.0,
                    h: chip.h - 3.0,
                },
                2.5,
                face,
            );
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
            .color(rgb(theme::TEXT_SUB.0, theme::TEXT_SUB.1, theme::TEXT_SUB.2));
        // 現在のピッチ: 発音中の音（最後に鳴らした音を優先）を、
        // 半音数と音名で大きく出す（Issue #26、デザイン文書の
        // ライブ感の要素）。
        if let Some(semi) = sampler::display_note(model.sampler_last, &active) {
            draw.text(&format!("{:+} st · {}", semi, sampler::note_name(semi)))
                .x_y(700.0 - win.w() / 2.0, y)
                .font_size(theme::VALUE_FONT_SIZE)
                .color(rgb(theme::TEXT.0, theme::TEXT.1, theme::TEXT.2));
        }
    }

    // 再生ヘッド（timeline.playhead）: 黒の細線＋現在の小節チップ
    // （Issue #24、デザイン文書「ライブ感の要素」）。
    let px = timeline::playhead_x(model.playhead_ms, win.w()) - win.w() / 2.0;
    draw.line()
        .start(pt2(px, win.h() / 2.0))
        .end(pt2(px, -win.h() / 2.0))
        .weight(1.5)
        .color(rgb(theme::TEXT.0, theme::TEXT.1, theme::TEXT.2));
    let chip_w = 48.0;
    let chip_h = 18.0;
    let chip_x = timeline::playhead_x(model.playhead_ms, win.w())
        .clamp(timeline::HEADER_WIDTH_PX, win.w() - chip_w);
    draw_rounded_rect(
        &draw,
        win,
        mixer::Rect {
            x: chip_x,
            y: 3.0,
            w: chip_w,
            h: chip_h,
        },
        theme::CHIP_CORNER_RADIUS,
        theme::ACCENT,
    );
    let (ccx, ccy) = rect_center(win, chip_x, 3.0, chip_w, chip_h);
    draw.text(&format!("BAR {}", timeline::bar_at_ms(model.playhead_ms)))
        .x_y(ccx, ccy)
        .font_size(11)
        .color(rgb(theme::PANEL.0, theme::PANEL.1, theme::PANEL.2));

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
    fn gesture_snapshot_skips_unchanged_state() {
        // Issue #31 レビュー対応: 動かさないクリック（無変化の逐次適用）
        // ではスナップショットは積まれず、redo も消えない。実際に
        // 変わった後の適用では1件だけ積まれる。
        let mut project = Project::new("gesture test");
        let track_id = apply(
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
                track_id,
                name: "c".into(),
                start_ms: 0,
                duration_ms: 1_000,
                source: "demo/c.wav".into(),
            },
        )
        .unwrap();
        let mut history = History::new(project);
        let clip_id = history.project().tracks[0].clips[0].id;
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
        // 積まれたのは開始時スナップショット（start 0）であること。
        assert!(history.undo());
        assert_eq!(history.project().tracks[0].clips[0].start_ms, 0);
    }

    #[test]
    fn gesture_returned_to_origin_records_nothing() {
        // Issue #31 レビュー round 2 対応: 一度動かして原位置へ戻した
        // ドラッグは、終了時の最終状態が開始状態と同じなので記録
        // されない（幽霊 undo を積まず、redo も消さない）。
        let mut project = Project::new("gesture return test");
        let track_id = apply(
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
                track_id,
                name: "c".into(),
                start_ms: 0,
                duration_ms: 1_000,
                source: "demo/c.wav".into(),
            },
        )
        .unwrap();
        let mut history = History::new(project);
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
        // 消費して 0 に戻り、500 が復元不能になる。
        let mut project = Project::new("gesture undo test");
        let track_id = apply(
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
                track_id,
                name: "c".into(),
                start_ms: 0,
                duration_ms: 1_000,
                source: "demo/c.wav".into(),
            },
        )
        .unwrap();
        let mut history = History::new(project);
        let clip_id = history.project().tracks[0].clips[0].id;
        // 確定編集: 0 → 500
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
        // handle_history_keys と同じ順序: ジェスチャ確定 → undo
        finish_gesture_snapshot(&mut history, &mut before);
        assert!(edit::apply_history_op(&mut history, edit::HistoryOp::Undo));
        assert_eq!(history.project().tracks[0].clips[0].start_ms, 500);
        assert!(edit::apply_history_op(&mut history, edit::HistoryOp::Redo));
        assert_eq!(history.project().tracks[0].clips[0].start_ms, 900);
    }

    #[test]
    fn pitch_label_color_is_theme_text_sub() {
        // Issue #28: ピッチ表示は小さい補助文字なので、旧来の金色
        // リテラル (0.95, 0.75, 0.35) ではなくテーマの TEXT_SUB
        // （グレージュ）を使う。色の決定は pitch_label_color() に
        // 集約し、ここで固定する。
        assert_eq!(pitch_label_color(), theme::TEXT_SUB);
        assert_ne!(pitch_label_color(), (0.95, 0.75, 0.35));
    }

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
