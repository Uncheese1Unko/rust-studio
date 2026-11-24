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
mod pianoroll;
mod plugin;
mod sampler;
mod tones;
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
    /// 音色操作（Issue #76）: チップのダブルクリック判定と改名
    /// セッション、右ボタンの押下エッジ追跡。どれも GUI の
    /// 表示状態で、プロジェクト状態には含めない。
    tone_clicks: edit::ClickTracker,
    tone_rename: Option<tones::ToneRenameSession>,
    prev_right_down: bool,
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
    // Issue #33: 複製・コピー＆ペーストのキー追跡とアプリ内
    // クリップボード（プロジェクトには保存しない）。
    edit_keys: edit::EditKeyTracker,
    clipboard: Option<studio_core::Clip>,
    // Issue #35: BPM のインライン入力セッション（プロジェクトには
    // 含めない）。
    bpm_edit: Option<edit::BpmEdit>,
    // Issue #36: メトロノームの ON/OFF（再生状態。プロジェクトには
    // 保存しない）。
    metronome_on: bool,
    // Issue #38: タイムラインの表示スケール（倍率と左端オフセット。
    // 表示状態で、プロジェクトには保存しない）。
    view_scale: timeline::ViewScale,
    // Issue #86: 再生ヘッドへの表示追従のオン／オフ（既定オン）と
    // F キーの押下エッジ追跡。表示状態であり、保存しない。
    follow_playhead: bool,
    prev_f_down: bool,
    // Issue #87: スクロールバーのドラッグ中は掴み位置のずれ（px）、
    // 中ボタンパンの最中は前フレームのポインタ x（px）を保持する。
    // いずれも表示状態であり、保存しない。
    scrollbar_drag: Option<f32>,
    pan_drag: Option<f32>,
    // Issue #40: ループ再生の範囲と ON/OFF（再生状態。プロジェクト
    // には保存しない）。OFF にすると範囲も消える。
    loop_range: Option<(i64, i64)>,
    loop_on: bool,
    // Issue #69: ピアノロール（開いているクリップと編集状態。
    // プロジェクトには保存しない GUI 状態）。roll_before は
    // ノートのドラッグジェスチャ開始時のスナップショット（#31 と
    // 同じ方式で、履歴への記録はジェスチャ終了時に行う）。
    piano_roll: Option<pianoroll::PianoRoll>,
    roll_before: Option<Project>,
    last_roll_cmd: Option<Command>,
    prev_note_delete_down: bool,
    /// ロールを開くダブルクリック判定（配置用 clicks とは分ける）。
    roll_clicks: edit::ClickTracker,
    // Issue #40: ルーラーで範囲をドラッグ中の始点と現在位置（ms）。
    loop_drag_anchor_ms: Option<i64>,
    loop_drag_pointer_ms: i64,
    prev_esc_down: bool,
    // Issue #41: WAV 書き出しの完了表示（画面と stdout に出す）。
    export_status: Option<String>,
    // Issue #72: プラグイントラックのレンダリング失敗（明示的な
    // エラー状態。空なら正常）。トラック名＋理由の文字列で持つ。
    plugin_errors: Vec<String>,
    // Issue #72: プラグイン関連状態の指紋の前回値。変化したら
    // 事前レンダリングをやり直す（レビュー Major 対応）。
    plugin_fingerprint: u64,
    // Issue #75: 指紋計算のファイルハッシュキャッシュ。毎フレームの
    // 指紋計算でプリセット／バイナリを読み直さないために保持する。
    file_hash_cache: plugin::FileHashCache,
    // Issue #42: 直近のドロップ処理時刻（秒）。複数ファイルの
    // ドロップは先頭1件だけ処理するためのバースト判定に使う。
    last_drop_at: Option<f32>,
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
/// 音色（Issue #73）の素材も含める: 外部から音色が追加・変更された
/// 再読込でも音声マップを作り直すため。
fn project_sources(project: &Project) -> Vec<String> {
    let mut sources: Vec<String> = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.source.clone()))
        .collect();
    for track in &project.tracks {
        if let Some(studio_core::Instrument::Sampler { source, .. }) =
            project.effective_instrument(track)
        {
            sources.push(source);
        }
    }
    for tone in &project.tones {
        if let studio_core::Instrument::Sampler { source, .. } = &tone.instrument {
            sources.push(source.clone());
        }
    }
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
/// ノートクリップの発音（Issue #68）にはトラックの楽器の素材も
/// 必要なので、楽器のソースも集合に含める。
/// プラグイン楽器（Issue #72）は別プロセスで事前レンダリングし、
/// 結果をマップへマージする。失敗はトラック単位のエラーとして
/// あわせて返す（呼び出し側が画面／stderr に明示する）。
fn compute_audio_map(
    project: &Project,
    dropped_source: &str,
) -> (
    HashMap<String, audio::ClipAudio>,
    Vec<plugin::PluginTrackError>,
) {
    let mut map = HashMap::new();
    let mut sources: Vec<String> = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.source.clone()))
        .collect();
    // 実効楽器（Issue #73: 音色割り当て込み）と、割り当てられて
    // いない音色の素材もデコードしておく（切り替えに即応するため）。
    for track in &project.tracks {
        if let Some(studio_core::Instrument::Sampler { source, .. }) =
            project.effective_instrument(track)
        {
            sources.push(source);
        }
    }
    for tone in &project.tones {
        if let studio_core::Instrument::Sampler { source, .. } = &tone.instrument {
            sources.push(source.clone());
        }
    }
    sources.push(dropped_source.to_string());
    sources.sort();
    sources.dedup();
    for source in sources {
        if let Ok(decoded) = audio::decode_audio(std::path::Path::new(&source)) {
            map.insert(source, decoded);
        }
    }
    let (plugin_audio, errors) =
        plugin::prepare_plugin_audio(project, &plugin::default_helper_path(), RATE);
    map.extend(plugin_audio);
    (map, errors)
}

/// プラグインエラーを画面表示用の文字列にする（Issue #72）。
fn format_plugin_errors(errors: &[plugin::PluginTrackError]) -> Vec<String> {
    errors
        .iter()
        .map(|e| format!("PLUGIN ERROR [{}] {}", e.track_name, e.message))
        .collect()
}

fn model(app: &App) -> Model {
    // Esc はループ解除（Issue #40）とリネーム／BPM 編集のキャンセル
    // （Issue #56/#35）で使うため、nannou 既定の「Esc で終了」を切る。
    app.set_exit_on_escape(false);
    let (tone220, tone440, dropped) = prepare_demo_media();
    let (project, project_file, file_stamp) = initial_project(&tone220, &tone440);
    println!("daw (issue #14: timeline UI)");
    println!("{}", project.dump_json());
    println!("elements: {}", timeline::element_ids(&project).join(", "));
    let (mut waveforms, mut source_durations) = (HashMap::new(), HashMap::new());
    // 配置用素材も波形を用意しておく（配置直後から波形が出るように）
    refresh_waveforms(&project, &dropped, &mut waveforms, &mut source_durations);
    let (audio_map, initial_plugin_errors) = compute_audio_map(&project, &dropped);
    let mut initial_cache = plugin::FileHashCache::new();
    let initial_fingerprint = plugin::plugin_fingerprint_cached(&project, &mut initial_cache);
    for error in &initial_plugin_errors {
        println!("plugin error [{}]: {}", error.track_name, error.message);
    }
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
        tone_clicks: edit::ClickTracker::new(),
        tone_rename: None,
        prev_right_down: false,
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
        edit_keys: edit::EditKeyTracker::new(),
        clipboard: None,
        bpm_edit: None,
        metronome_on: false,
        view_scale: timeline::ViewScale::default(),
        follow_playhead: true,
        prev_f_down: false,
        scrollbar_drag: None,
        pan_drag: None,
        loop_range: None,
        loop_on: false,
        piano_roll: None,
        roll_before: None,
        last_roll_cmd: None,
        prev_note_delete_down: false,
        roll_clicks: edit::ClickTracker::new(),
        loop_drag_anchor_ms: None,
        loop_drag_pointer_ms: 0,
        prev_esc_down: false,
        export_status: None,
        plugin_errors: format_plugin_errors(&initial_plugin_errors),
        plugin_fingerprint: initial_fingerprint,
        file_hash_cache: initial_cache,
        last_drop_at: None,
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
    // WAV 書き出し（Issue #41 の検証用 CLI）:
    // daw --export-wav <project.json> <out.wav>
    if args.len() == 4 && args[1] == "--export-wav" {
        export_wav_cli(&args[2], &args[3]);
        return;
    }
    // MIDI 取り込み（Issue #70 の検証用 CLI）:
    // daw --import-midi <project.json> <file.mid>
    if args.len() == 4 && args[1] == "--import-midi" {
        import_midi_cli(&args[2], &args[3]);
        return;
    }
    nannou::app(model).event(event).update(update).run();
}

/// プロジェクト JSON へ MIDI ファイルを取り込んで保存し直す
/// （Issue #70）。取り込みは studio-core の import_midi（履歴には
/// 載らないが、保存されるプロジェクトは GUI／MCP と同じ形）。
fn import_midi_cli(project_json: &str, mid_path: &str) {
    let project_path = std::path::Path::new(project_json);
    let project = Project::load_from_path(project_path).expect("load project json");
    let bytes = std::fs::read(mid_path).expect("read midi file");
    let mut history = studio_core::History::new(project);
    match studio_core::import_midi(&mut history, &bytes) {
        Ok(summary) => {
            history
                .project()
                .save_to_path(project_path)
                .expect("save project json");
            println!(
                "midi imported: {} tracks, {} notes, bpm {:?}",
                summary.tracks_added, summary.notes_added, summary.bpm
            );
            println!("dump: {}", history.project().dump_json());
        }
        Err(e) => {
            eprintln!("midi import failed: {e}");
            std::process::exit(1);
        }
    }
}

/// 音声マップの再計算とプラグイン指紋の同期（Issue #75 候補1）。
/// `compute_audio_map` を呼ぶ箇所はすべてここを通し、直後の
/// `update()` が指紋不一致を検知して二重に再レンダリングする
/// 無駄を無くす。指紋はキャッシュ付きで計算する（候補2）。
fn refresh_audio_map(model: &mut Model) {
    let (map, errors) = compute_audio_map(model.history.project(), &model.dropped_source);
    model.audio_map = map;
    model.plugin_errors = format_plugin_errors(&errors);
    model.plugin_fingerprint =
        plugin::plugin_fingerprint_cached(model.history.project(), &mut model.file_hash_cache);
}

/// プロジェクト JSON を読み、ミックスを WAV に書き出す（Issue #41）。
fn export_wav_cli(project_json: &str, out_wav: &str) {
    let project =
        Project::load_from_path(std::path::Path::new(project_json)).expect("load project json");
    let (audio_map, plugin_errors) = compute_audio_map(&project, "");
    for error in &plugin_errors {
        eprintln!("plugin error [{}]: {}", error.track_name, error.message);
    }
    match audio::export_wav(&project, &audio_map, std::path::Path::new(out_wav)) {
        Ok(path) => println!("exported: {}", path.display()),
        Err(e) => {
            eprintln!("export failed: {e}");
            std::process::exit(1);
        }
    }
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
            // ピアノロールも打ち切る（Issue #69 レビュー指摘）:
            // ノート構成が変わると選択・ドラッグのインデックスが
            // 別のノートを指し、旧プロジェクトの roll_before が
            // 新しい History に記録され得るため、ロールごと閉じる。
            model.piano_roll = None;
            model.roll_before = None;
            model.last_roll_cmd = None;
            // リネーム編集も打ち切る（バグ #60: 対象が消えると Enter の
            // たびに拒否され続け、残っていても外部変更後の名前を編集中
            // テキストで黙って上書きしてしまうため）。
            model.rename = None;
            model.tone_rename = None;
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
                refresh_audio_map(model);
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
/// ホイール操作（Issue #38）: Ctrl+ホイールでマウス位置の時刻を
/// 中心にズーム、通常のホイールで水平スクロールする。テキスト入力中は
/// 受け付けない。倍率とオフセットは表示状態で、保存はしない。
/// ファイルドロップの取り込み（Issue #42）。配置先は選択中トラック＋
/// 再生ヘッド位置（#30 調査: ドロップ座標は取得できない）。対応外・
/// デコード不能・配置不能は何もせず理由をログに出す。複数ファイルの
/// ドロップは先頭1件だけ処理し、残りはログに出す（1秒以内の連続
/// ドロップを同じバーストとみなす）。
/// MIDI ファイルを GUI のプロジェクトへ取り込む（Issue #70）。
/// 取り込みは履歴に1件積まれ、Undo で戻せる。構造が変わるため
/// 開いているピアノロールは閉じる（再読み込み時と同じ扱い）。
fn import_midi_file(model: &mut Model, path: &std::path::Path) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            println!(
                "midi import rejected: {} (cannot read: {e})",
                path.display()
            );
            return;
        }
    };
    match studio_core::import_midi(&mut model.history, &bytes) {
        Ok(summary) => {
            model.piano_roll = None;
            model.roll_before = None;
            model.last_roll_cmd = None;
            save_project_file(model);
            println!(
                "midi imported: {} ({} tracks, {} notes, bpm {:?})",
                path.display(),
                summary.tracks_added,
                summary.notes_added,
                summary.bpm
            );
            println!("after midi import: {}", model.history.project().dump_json());
        }
        Err(e) => {
            println!("midi import rejected: {} ({e})", path.display());
        }
    }
}

fn handle_drop(app: &App, model: &mut Model, path: &std::path::Path) {
    // Issue #70: .mid / .midi はオーディオ配置ではなく MIDI
    // 取り込みとして扱う（重複抑制の対象外: 取り込みは明示操作）。
    let is_midi = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mid") || e.eq_ignore_ascii_case("midi"));
    if is_midi {
        import_midi_file(model, path);
        return;
    }
    let now = app.time;
    if let Some(last) = model.last_drop_at {
        if now - last < 1.0 {
            println!(
                "drop skipped: {} (only the first file of a drop is imported)",
                path.display()
            );
            return;
        }
    }
    model.last_drop_at = Some(now);
    if !edit::is_supported_drop_path(path) {
        println!(
            "drop rejected: {} (unsupported file type; only .wav)",
            path.display()
        );
        return;
    }
    let decoded = match audio::decode_audio(path) {
        Ok(d) => d,
        Err(e) => {
            println!("drop rejected: {} (cannot decode: {e})", path.display());
            return;
        }
    };
    let source = path.to_string_lossy().into_owned();
    let cmd = match edit::drop_placement_command(
        model.history.project(),
        model.selected_track,
        model.playhead_ms,
        &source,
        decoded.duration_ms(),
    ) {
        Some(cmd) => cmd,
        None => {
            println!(
                "drop rejected: {} (no track to place on or empty audio)",
                path.display()
            );
            return;
        }
    };
    match model.history.apply(&cmd) {
        Ok(_) => {
            model.dropped_source = source;
            refresh_waveforms(
                model.history.project(),
                &model.dropped_source,
                &mut model.waveforms,
                &mut model.source_durations,
            );
            refresh_audio_map(model);
            save_project_file(model);
            println!(
                "dropped: {} -> {}",
                path.display(),
                model.history.project().dump_json()
            );
        }
        Err(e) => println!("drop rejected: {} (command rejected: {e})", path.display()),
    }
}

/// ホイール操作: Ctrl+ホイールでマウス位置の時刻を中心にズーム、
/// 通常のホイールで水平スクロールする。リネーム／BPM 編集中は
/// 受け付けない。倍率とオフセットは表示状態で、保存はしない。
/// Issue #87: スクロール量は表示幅の 1/10 /ノッチ（ズームに比例）、
/// 横ホイールと Shift+ホイールでもスクロールし、オフセットは
/// プロジェクト終端でクランプする。
fn mouse_wheel(app: &App, model: &mut Model, delta: MouseScrollDelta) {
    if model.rename.is_some() || model.bpm_edit.is_some() || model.tone_rename.is_some() {
        return;
    }
    // 横成分（x）も捨てずに拾う。PixelDelta は既存の換算
    // （50px = 1行）を両成分に適用する。
    let (x_lines, y_lines) = match delta {
        MouseScrollDelta::LineDelta(x, y) => (x, y),
        MouseScrollDelta::PixelDelta(pos) => (pos.x as f32 / 50.0, pos.y as f32 / 50.0),
    };
    if x_lines == 0.0 && y_lines == 0.0 {
        return;
    }
    let win = app.window_rect();
    let mx = app.mouse.x + win.w() / 2.0;
    if ctrl_down(app) && y_lines != 0.0 {
        let anchor_ms = model.view_scale.x_to_ms(mx, win.w());
        let factor = if y_lines > 0.0 { 1.25 } else { 0.8 };
        model.view_scale = model.view_scale.zoom_at(factor, anchor_ms, mx, win.w());
        println!(
            "zoom: {:.2} offset: {}",
            model.view_scale.zoom, model.view_scale.offset_ms
        );
    } else {
        let delta_ms = model.view_scale.wheel_delta_ms(x_lines, y_lines, win.w());
        let end = timeline::project_end_ms(model.history.project());
        let next = model.view_scale.scroll_by_clamped(delta_ms, end, win.w());
        if next.offset_ms != model.view_scale.offset_ms {
            model.view_scale = next;
            println!("scroll offset: {}", model.view_scale.offset_ms);
        }
    }
}

fn event(app: &App, model: &mut Model, event: Event) {
    // ファイルドロップ（Issue #42）はテキスト入力の有無によらず先に
    // 捌く。パスだけが届き、座標は取得できない（#30 調査）。
    if let Event::WindowEvent {
        simple: Some(WindowEvent::DroppedFile(path)),
        ..
    } = &event
    {
        let path = path.clone();
        handle_drop(app, model, &path);
        return;
    }
    // ホイール（Issue #38）はテキスト入力の有無によらず先に捌く
    // （入力中は mouse_wheel 側で無視する）。
    if let Event::WindowEvent {
        simple: Some(WindowEvent::MouseWheel(delta, _)),
        ..
    } = event
    {
        mouse_wheel(app, model, delta);
        return;
    }
    if model.rename.is_none() && model.bpm_edit.is_none() && model.tone_rename.is_none() {
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
    // BPM 入力（Issue #35）: リネームとは同時に開かない。
    if model.rename.is_none() {
        // 音色の改名（Issue #76）: BPM 入力とも同時に開かない。
        if let Some(session) = model.tone_rename.as_mut() {
            match session.handle(input) {
                edit::RenameAction::None => {}
                edit::RenameAction::Cancel => {
                    model.tone_rename = None;
                    println!("tone rename cancelled");
                }
                edit::RenameAction::Commit(cmd) => match model.history.apply(&cmd) {
                    Ok(_) => {
                        model.tone_rename = None;
                        println!("after tone rename: {}", model.history.project().dump_json());
                        save_project_file(model);
                    }
                    Err(e) => println!("tone rename rejected: {e}"),
                },
            }
            return;
        }
        if let Some(session) = model.bpm_edit.as_mut() {
            match session.handle(input) {
                edit::BpmAction::None => {}
                edit::BpmAction::Cancel => {
                    model.bpm_edit = None;
                    println!("bpm edit cancelled");
                }
                edit::BpmAction::Commit(cmd) => match model.history.apply(&cmd) {
                    Ok(_) => {
                        model.bpm_edit = None;
                        println!("after bpm: {}", model.history.project().dump_json());
                        save_project_file(model);
                    }
                    Err(e) => println!("bpm rejected: {e}"),
                },
            }
        }
        return;
    }
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
    // 受け付けない（Issue #56）。BPM 入力中も同じ扱い（Issue #35）。
    // 再生ヘッドとファイル監視は続ける。
    let renaming =
        model.rename.is_some() || model.bpm_edit.is_some() || model.tone_rename.is_some();
    if renaming {
        model.prev_left_down = app.mouse.buttons.left().is_down();
        // Issue #76 レビュー指摘: 右ボタンのエッジ追跡も同期する
        // （編集中に押した右クリックで、編集終了直後にチップの
        // 音色削除が誤発火しないように。バグ #60 と同じ型）。
        model.prev_right_down = app.mouse.buttons.right().is_down();
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
        model.edit_keys.sync(
            app.keys.down.contains(&Key::D),
            app.keys.down.contains(&Key::C),
            app.keys.down.contains(&Key::V),
        );
        model.prev_note_delete_down =
            app.keys.down.contains(&Key::Delete) || app.keys.down.contains(&Key::Back);
        // Issue #86: 追従切り替えの F も編集中は同期だけ行う
        // （編集中に押した F で編集終了直後に追従が切り替わらない
        // ように。バグ #60 と同じパターン）。
        model.prev_f_down = app.keys.down.contains(&Key::F);
    } else {
        handle_pointer(app, model);
    }
    watch_project_file(app, model);
    if !renaming {
        handle_history_keys(app, model);
        handle_roll_keys(app, model);
        handle_edit_keys(app, model);
        handle_pitch_keys(app, model);
        // 再生追従の切り替え（Issue #86）: F キーの押下エッジ。
        // F はサンプラーのノートキーでもあるが、切り替えは表示状態
        // のみで、発音への影響はない。
        let f_down = app.keys.down.contains(&Key::F);
        if f_down && !model.prev_f_down {
            model.follow_playhead = !model.follow_playhead;
            println!(
                "follow: {}",
                if model.follow_playhead { "on" } else { "off" }
            );
        }
        model.prev_f_down = f_down;
    }
    // Issue #72 レビュー対応: プラグイン関連の状態（楽器・
    // プリセット・ノート・クリップ配置）が変わったら事前
    // レンダリングをやり直す。ピアノロール編集・タイムライン
    // 編集・Undo/Redo・外部再読込の全経路をここで一括りにする。
    // ドラッグ中（左ボタン押下中）は確定まで遅らせる。
    let fp = plugin::plugin_fingerprint_cached(model.history.project(), &mut model.file_hash_cache);
    if fp != model.plugin_fingerprint && !app.mouse.buttons.left().is_down() {
        // refresh_audio_map が指紋も同期する（Issue #75 候補1）。
        refresh_audio_map(model);
        for error in &model.plugin_errors {
            println!("{error}");
        }
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
        } else if !handle_split_key(app, model) {
            handle_sampler_keys(app, model);
        }
    }
    if let Some(sampler) = &mut model.sampler {
        sampler.advance(dt_ms);
    }
    let end = timeline::project_end_ms(model.history.project());
    let prev_playhead_ms = model.playhead_ms;
    model.playhead_ms = timeline::advance_playhead_in_loop(
        model.playhead_ms,
        dt_ms,
        end,
        model.loop_range,
        model.loop_on,
    );
    // 再生追従（Issue #86）: 追従オンのとき、再生ヘッドが表示右端を
    // 超えたらページ送りで表示を進める。巻き戻り（終端ラップと
    // ループ範囲の折り返しはどちらも「新しい再生ヘッド < 直前」で
    // 検出できる）では、折り返し先を含むページへオフセットを戻す。
    // オフセットが変わったらホイールスクロールと同じ形式で標準
    // 出力に記録する。
    if model.follow_playhead {
        let wrapped = model.playhead_ms < prev_playhead_ms;
        let win = app.window_rect();
        let next = model
            .view_scale
            .follow_playhead(model.playhead_ms, win.w(), wrapped);
        if next.offset_ms != model.view_scale.offset_ms {
            model.view_scale = next;
            println!("scroll offset: {}", model.view_scale.offset_ms);
        }
    }
    // Esc の処理: ピアノロールが開いていればまずそれを閉じ（Issue
    // #69）、次にループ解除（Issue #40）。テキスト編集中の Esc は
    // 編集のキャンセルが優先する（レビュー所見への対応）。
    let esc_down = app.keys.down.contains(&Key::Escape);
    if esc_down
        && !model.prev_esc_down
        && model.rename.is_none()
        && model.bpm_edit.is_none()
        && model.tone_rename.is_none()
    {
        if model.piano_roll.is_some() {
            // ドラッグ中に閉じる場合はジェスチャを確定してから
            // 閉じる（transient 適用済みの移動を、履歴に残らず
            // Undo で戻せない状態で確定させない。Issue #69
            // レビュー指摘）。
            let had_drag = model
                .piano_roll
                .as_mut()
                .map(|r| r.drag.take().is_some())
                .unwrap_or(false);
            if had_drag {
                finish_gesture_snapshot(&mut model.history, &mut model.roll_before);
                save_project_file(model);
            }
            model.piano_roll = None;
            model.roll_before = None;
            model.last_roll_cmd = None;
            println!("piano roll: closed");
        } else if model.loop_on || model.loop_range.is_some() {
            model.loop_on = false;
            model.loop_range = None;
            println!("loop: cleared");
        }
    }
    model.prev_esc_down = esc_down;
    // レベルメーター（Issue #27）: 再生位置の直後のミックスを
    // レンダリングしてピークを取る（読み取り専用。音声処理には
    // 手を入れない）。再生位置が 50ms 以上動いたときだけ再計算する。
    if (model.playhead_ms - model.level_at_ms).abs() >= 50 {
        let mix = audio::render_mix_with_metronome(
            model.history.project(),
            &model.audio_map,
            model.playhead_ms,
            LEVEL_WINDOW_MS,
            44_100,
            model.metronome_on,
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
    // 進行中ジェスチャ（ドラッグ／ミキサー）への配慮は
    // `edit::apply_history_key` に集約されている（Undo は先に
    // ジェスチャを確定させ、Redo はジェスチャに触れずに試す。
    // #31 レビュー round 3・4 対応）。成功したときだけ後始末する。
    let applied = edit::apply_history_key(
        &mut model.history,
        op,
        &mut [
            &mut model.drag_before,
            &mut model.mixer_before,
            &mut model.roll_before,
        ],
    );
    if !applied {
        return;
    }
    model.drag = None;
    model.last_drag_cmd = None;
    model.last_rejection = None;
    model.drag_before = None;
    model.mixer_active = None;
    model.last_mixer_cmd = None;
    model.mixer_before = None;
    model.roll_before = None;
    // ピアノロール（Issue #69）: 履歴が動いたらドラッグと選択を
    // 打ち切り、対象クリップが消えていたら閉じる。
    model.last_roll_cmd = None;
    let roll_clip_gone = match &mut model.piano_roll {
        Some(roll) => {
            roll.drag = None;
            roll.selected = None;
            model.history.project().clip(roll.clip_id).is_none()
        }
        None => false,
    };
    if roll_clip_gone {
        model.piano_roll = None;
    }
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
    refresh_audio_map(model);
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

/// 複製・コピー＆ペースト（Issue #33）: Ctrl+D / Ctrl+C / Ctrl+V。
/// 対象クリップは、ピッチ・色・分割と同じく「選択中トラックの
/// 再生ヘッド位置にあるクリップ」。複製は元クリップの直後、
/// 貼り付けは選択中トラックの再生ヘッド位置に置く。配置は
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
    if model.drag.is_some() || model.mixer_active.is_some() {
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
            apply_placement(model, &placement, "duplicate");
        }
        edit::EditOp::Paste => {
            let (Some(clipboard), Some(track_id)) = (model.clipboard.clone(), model.selected_track)
            else {
                println!("paste: clipboard is empty or no track selected");
                return;
            };
            let placement = edit::paste_placement(&clipboard, track_id, model.playhead_ms);
            apply_placement(model, &placement, "paste");
        }
    }
}

/// 配置計画（AddClip＋パラメータ設定）を履歴経由で適用する
/// （Issue #33）。新しいクリップができたら波形・音声マップを
/// 追従させ、プロジェクトファイルへ保存する。
fn apply_placement(model: &mut Model, placement: &edit::ClipPlacement, label: &str) {
    let new_id = match model.history.apply(&placement.add) {
        Ok(Some(id)) => id,
        Ok(None) => {
            println!("{label}: AddClip returned no id");
            return;
        }
        Err(e) => {
            println!("{label} rejected: {e}");
            return;
        }
    };
    for cmd in placement.follow_up_commands(new_id) {
        if let Err(e) = model.history.apply(&cmd) {
            println!("{label} follow-up rejected: {e}");
        }
    }
    refresh_waveforms(
        model.history.project(),
        &model.dropped_source,
        &mut model.waveforms,
        &mut model.source_durations,
    );
    refresh_audio_map(model);
    println!("after {label}: {}", model.history.project().dump_json());
    save_project_file(model);
}

/// 分割（Issue #32）: S キーの押下エッジで、選択中トラックの
/// 再生ヘッド位置にあるクリップを再生ヘッド位置で分割する。
/// S はサンプラーのノートキーと重なるため、再生ヘッドがクリップの
/// 内部（端でない位置）にある場合だけこちらがキーを消費し、
/// それ以外は従来どおりサンプラー演奏に渡す（イシューの実装方針）。
/// 戻り値: このフレームで S を消費したら true。
fn handle_split_key(app: &App, model: &mut Model) -> bool {
    let s_idx = NOTE_KEYS
        .iter()
        .position(|(k, _)| *k == Key::S)
        .expect("S is a note key");
    let down = app.keys.down.contains(&Key::S);
    if !down || model.prev_note_keys[s_idx] {
        return false;
    }
    let target = model
        .selected_track
        .and_then(|tid| model.history.project().track(tid))
        .and_then(|t| {
            t.clips
                .iter()
                .find(|c| model.playhead_ms > c.start_ms && model.playhead_ms < c.end_ms())
        })
        .map(|c| c.id);
    let Some(clip_id) = target else {
        return false;
    };
    // サンプラー側の発火を抑止する（prev を現在値に同期）。
    model.prev_note_keys[s_idx] = true;
    let cmd = Command::SplitClip {
        clip_id,
        at_ms: model.playhead_ms,
    };
    match model.history.apply(&cmd) {
        Ok(_) => {
            println!("after split: {}", model.history.project().dump_json());
            save_project_file(model);
        }
        Err(e) => println!("split rejected: {e}"),
    }
    true
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

/// 音色パネルのクリックをコマンドに変換する（Issue #73、ADR-0018）。
/// チップは選択中トラックへの割り当て／解除、作成は選択中トラックの
/// 実効楽器から取り込み、削除は選択中トラックに割り当て中の音色を
/// 対象とする。状態変更はコマンド経由のみ（ADR-0004）。音色の変更は
/// 音声マップに影響するため、適用後に作り直す。
fn apply_tone_click(model: &mut Model, hit: tones::ToneHit) {
    let Some(track_id) = model.selected_track else {
        println!("tone: no track selected");
        return;
    };
    let project = model.history.project();
    let cmd = match hit {
        tones::ToneHit::Chip(index) => {
            let Some(tone) = project.tones.get(index) else {
                return;
            };
            if project.track(track_id).and_then(|t| t.tone_id) == Some(tone.id) {
                Command::SetTrackTone {
                    track_id,
                    tone_id: None,
                }
            } else {
                Command::SetTrackTone {
                    track_id,
                    tone_id: Some(tone.id),
                }
            }
        }
        tones::ToneHit::Add => {
            let Some(track) = project.track(track_id) else {
                return;
            };
            let Some(instrument) = project.effective_instrument(track) else {
                println!("tone: selected track has no instrument to capture");
                return;
            };
            let base = match &instrument {
                studio_core::Instrument::Sampler { source, .. } => {
                    source.rsplit('/').next().unwrap_or(source).to_string()
                }
                studio_core::Instrument::Plugin { plugin_path, .. } => plugin_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(plugin_path)
                    .to_string(),
            };
            let name = if base.trim().is_empty() {
                format!("Tone {}", project.tones.len() + 1)
            } else {
                base
            };
            Command::AddTone { name, instrument }
        }
        tones::ToneHit::Delete => {
            let Some(tone_id) = project.track(track_id).and_then(|t| t.tone_id) else {
                println!("tone: selected track has no tone assigned");
                return;
            };
            Command::RemoveTone { tone_id }
        }
    };
    match model.history.apply(&cmd) {
        Ok(_) => {
            println!("after tone: {}", model.history.project().dump_json());
            save_project_file(model);
            refresh_audio_map(model);
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

/// ピアノロールを開けるクリップかどうか（Issue #69）。
/// ノートクリップと素材なしクリップは常に対象。素材のある
/// クリップは、楽器のあるトラックのものだけを意図的な導線として
/// 対象にする（この場合、最初のノート配置でそのクリップは
/// ノートクリップになり波形は鳴らなくなる。ADR-0016 の意味論
/// どおりで、ダブルクリックという明示操作を変換点とする）。
/// 楽器のないトラックの波形クリップは対象外（誤ってノート
/// クリップ化しないため）。
fn roll_openable(project: &Project, clip_id: u64) -> bool {
    let Some((track, clip)) = project.clip(clip_id) else {
        return false;
    };
    !clip.notes.is_empty()
        || clip.source.is_empty()
        || project.effective_instrument(track).is_some()
}

/// ピアノロールのキー操作（Issue #69）: Delete / Backspace で
/// 選択中のノートを削除する。押下エッジでのみ発火する。
fn handle_roll_keys(app: &App, model: &mut Model) {
    let delete_down = app.keys.down.contains(&Key::Delete) || app.keys.down.contains(&Key::Back);
    if delete_down && !model.prev_note_delete_down && model.piano_roll.is_some() {
        let (clip_id, selected) = {
            let roll = model.piano_roll.as_ref().expect("checked above");
            (roll.clip_id, roll.selected)
        };
        if let Some(note_index) = selected {
            let cmd = Command::RemoveNote {
                clip_id,
                note_index,
            };
            match model.history.apply(&cmd) {
                Ok(_) => {
                    if let Some(roll) = model.piano_roll.as_mut() {
                        roll.selected = None;
                    }
                    println!("after note remove: {}", model.history.project().dump_json());
                    save_project_file(model);
                }
                Err(e) => println!("command rejected: {e}"),
            }
        }
    }
    model.prev_note_delete_down = delete_down;
}

/// ピアノロール表示中のポインタ操作（Issue #69）。クリックで配置、
/// ノートのドラッグで移動（ジェスチャ終了時に履歴へ記録、#31 と
/// 同じ方式）。状態変更はすべてコマンド層経由。
fn handle_roll_pointer(app: &App, model: &mut Model, lx: f32, ly: f32, left_down: bool) {
    let win = app.window_rect();
    let clip_id = model
        .piano_roll
        .as_ref()
        .map(|r| r.clip_id)
        .expect("roll open");
    // 対象クリップが消えていたら（Undo で削除された等）閉じる。
    let Some((_, clip)) = model.history.project().clip(clip_id) else {
        model.piano_roll = None;
        model.roll_before = None;
        model.last_roll_cmd = None;
        println!("piano roll: closed (clip gone)");
        return;
    };
    let clip = clip.clone();

    if left_down && !model.prev_left_down {
        // ドラッグ中の再押下は無視する（ドラッグは解放で終わる。
        // 押下の取りこぼし・跳ね返りで配置が走るのを防ぐ）。
        if model.piano_roll.as_ref().is_some_and(|r| r.drag.is_some()) {
            return;
        }
        let roll = model.piano_roll.as_ref().expect("roll open");
        if let Some(index) = roll.hit_note(win.w(), win.h(), &clip, lx, ly) {
            let drag = roll.begin_drag(win.w(), win.h(), &clip, index, lx, ly);
            let roll = model.piano_roll.as_mut().expect("roll open");
            roll.selected = Some(index);
            roll.drag = drag;
            model.roll_before = Some(model.history.project().clone());
            model.last_roll_cmd = None;
        } else if let Some(cmd) = roll.place_command(win.w(), win.h(), &clip, lx, ly) {
            match model.history.apply(&cmd) {
                Ok(_) => {
                    let placed_index = model
                        .history
                        .project()
                        .clip(clip_id)
                        .map(|(_, c)| c.notes.len().saturating_sub(1));
                    if let Some(roll) = model.piano_roll.as_mut() {
                        roll.selected = placed_index;
                    }
                    println!("after note add: {}", model.history.project().dump_json());
                    save_project_file(model);
                }
                Err(e) => println!("command rejected: {e}"),
            }
        } else if let Some(roll) = model.piano_roll.as_mut() {
            roll.selected = None;
        }
    }

    if left_down {
        let drag = model.piano_roll.as_ref().and_then(|r| r.drag.clone());
        if let Some(drag) = drag {
            let roll = model.piano_roll.as_ref().expect("roll open");
            let cmd = drag.command_for(roll, win.w(), win.h(), &clip, lx, ly);
            if model.last_roll_cmd.as_ref() != Some(&cmd) {
                match model.history.apply_transient(&cmd) {
                    Ok(_) => {
                        model.last_roll_cmd = Some(cmd);
                    }
                    Err(e) => println!("command rejected: {e}"),
                }
            }
        }
    }

    if !left_down && model.prev_left_down {
        let had_drag = model
            .piano_roll
            .as_mut()
            .map(|r| r.drag.take().is_some())
            .unwrap_or(false);
        if had_drag {
            // ジェスチャ終了時にだけ履歴へ記録する（Issue #31）。
            finish_gesture_snapshot(&mut model.history, &mut model.roll_before);
            println!("after note move: {}", model.history.project().dump_json());
            save_project_file(model);
            model.last_roll_cmd = None;
        }
    }
}

/// ポインタ操作をポーリングし、コマンド経由で編集する（Issue #14、
/// video-editor と同じ意味論）。押下でドラッグ開始（またはダブルクリック
/// で配置）、ドラッグ中は候補コマンドが変わるたびに適用し、解放時に
/// 状態ダンプを出す。範囲外の候補はコマンド層が弾く。
/// スクロールバーのドラッグを表示オフセットへ反映する（Issue #87）。
/// 換算とクランプは timeline の純粋関数に寄せる。
fn apply_scrollbar_drag(model: &mut Model, lx: f32, view_width_px: f32) {
    let Some(grab_dx) = model.scrollbar_drag else {
        return;
    };
    let end = timeline::project_end_ms(model.history.project());
    let next = model
        .view_scale
        .scrollbar_drag_to(lx - grab_dx, end, view_width_px);
    if next.offset_ms != model.view_scale.offset_ms {
        model.view_scale = next;
        println!("scroll offset: {}", model.view_scale.offset_ms);
    }
}

fn handle_pointer(app: &App, model: &mut Model) {
    let win = app.window_rect();
    // nannou の中心原点座標を、レイアウトの左上原点ピクセルへ変換する。
    let lx = app.mouse.x + win.w() / 2.0;
    let ly = win.h() / 2.0 - app.mouse.y;
    let left_down = app.mouse.buttons.left().is_down();
    let right_down = app.mouse.buttons.right().is_down();
    let pointer_ms = edit::x_to_ms(lx, win.w(), &model.view_scale);

    // ピアノロール（Issue #69）が開いている間は、ポインタ操作を
    // すべてロール側で捌く（タイムラインとは画面モードが別）。
    if model.piano_roll.is_some() {
        handle_roll_pointer(app, model, lx, ly, left_down);
        model.prev_left_down = left_down;
        model.prev_right_down = right_down;
        return;
    }

    // 音色チップの右クリック（Issue #76）: 割り当て中かどうかを
    // 問わず、その音色を削除する。履歴経由なので Undo で戻せる。
    if right_down && !model.prev_right_down {
        let tones_snapshot = model.history.project().tones.clone();
        if let Some(tones::ToneHit::Chip(index)) = tones::hit_tones(
            model.history.project().tracks.len(),
            &tones_snapshot,
            lx,
            ly,
            win.w() - 16.0,
        ) {
            if let Some(tone) = tones_snapshot.get(index) {
                let cmd = Command::RemoveTone { tone_id: tone.id };
                match model.history.apply(&cmd) {
                    Ok(_) => {
                        println!("after tone: {}", model.history.project().dump_json());
                        save_project_file(model);
                        refresh_audio_map(model);
                    }
                    Err(e) => println!("command rejected: {e}"),
                }
            }
        }
    }
    model.prev_right_down = right_down;

    // スクロールバー（Issue #87）: 帯での左押下でドラッグ開始し、
    // 保持中はつまみをポインタに追従させる。帯は他の要素と
    // 重ならないため、左ボタンの他の処理より先に捌く。
    if left_down && !model.prev_left_down {
        let end = timeline::project_end_ms(model.history.project());
        if let Some(grab_dx) = timeline::scrollbar_grab(
            &model.view_scale,
            end,
            model.history.project().tracks.len(),
            lx,
            ly,
            win.w(),
        ) {
            model.scrollbar_drag = Some(grab_dx);
            apply_scrollbar_drag(model, lx, win.w());
            model.prev_left_down = left_down;
            return;
        }
    }
    if left_down && model.scrollbar_drag.is_some() {
        apply_scrollbar_drag(model, lx, win.w());
    }
    if !left_down && model.scrollbar_drag.take().is_some() {
        println!("scrollbar drag end: offset {}", model.view_scale.offset_ms);
    }
    // 中ボタンドラッグでのパン（Issue #87）: 掴んだ内容がポインタに
    // 追従する向きにする（左へ引くと先の内容が見える）。移動量は
    // px を現在の倍率で ms に換算し、終端でクランプする。
    let middle_down = app.mouse.buttons.middle().is_down();
    if middle_down {
        if let Some(last_lx) = model.pan_drag {
            let dx_px = last_lx - lx;
            if dx_px != 0.0 {
                let delta_ms = (dx_px / model.view_scale.px_per_ms(win.w())).round() as i64;
                let end = timeline::project_end_ms(model.history.project());
                let next = model.view_scale.scroll_by_clamped(delta_ms, end, win.w());
                if next.offset_ms != model.view_scale.offset_ms {
                    model.view_scale = next;
                    println!("scroll offset: {}", model.view_scale.offset_ms);
                }
            }
        }
        model.pan_drag = Some(lx);
    } else {
        model.pan_drag = None;
    }

    // ミキサー（Issue #15）: スライダーの押下・ドラッグで音量／パンを
    // コマンド経由で変える。タイムライン領域とは重ならない。
    if left_down && !model.prev_left_down {
        // M/S ボタン（Issue #39）: 押下で現在状態を反転させる。
        if let Some(hit) = mixer::hit_mixer_button(model.history.project(), lx, ly) {
            if let Some(cmd) = mixer::toggle_command(model.history.project(), &hit) {
                match model.history.apply(&cmd) {
                    Ok(_) => {
                        save_project_file(model);
                        println!(
                            "after mixer button: {}",
                            model.history.project().dump_json()
                        );
                    }
                    Err(e) => println!("command rejected: {e}"),
                }
            }
        }
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
        // メトロノームトグル（Issue #36）: アプリ側の再生状態を切り替える。
        let m_r = metronome_rect(model.history.project().tracks.len());
        if lx >= m_r.x && lx < m_r.x + m_r.w && ly >= m_r.y && ly < m_r.y + m_r.h {
            model.metronome_on = !model.metronome_on;
            println!(
                "metronome: {}",
                if model.metronome_on { "on" } else { "off" }
            );
            model.prev_left_down = left_down;
            return;
        }
        // ループトグル（Issue #40）: OFF にすると範囲も消える。
        let l_r = loop_rect(model.history.project().tracks.len());
        if lx >= l_r.x && lx < l_r.x + l_r.w && ly >= l_r.y && ly < l_r.y + l_r.h {
            model.loop_on = !model.loop_on;
            if !model.loop_on {
                model.loop_range = None;
            }
            println!("loop: {}", if model.loop_on { "on" } else { "off" });
            model.prev_left_down = left_down;
            return;
        }
        // WAV 書き出し（Issue #41）: 初期値の書き出し先へ書く。
        let e_r = export_rect(model.history.project().tracks.len());
        if lx >= e_r.x && lx < e_r.x + e_r.w && ly >= e_r.y && ly < e_r.y + e_r.h {
            let out = audio::default_export_path(
                model.project_file.as_deref(),
                &model.history.project().name,
            );
            match audio::export_wav(model.history.project(), &model.audio_map, &out) {
                Ok(path) => {
                    // チップにはファイル名だけ出す（絶対パスは stdout）。
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string());
                    model.export_status = Some(format!("EXPORTED {name}"));
                    println!("exported: {}", path.display());
                }
                Err(e) => {
                    model.export_status = Some("EXPORT FAILED".to_string());
                    println!("export failed: {e}");
                }
            }
            model.prev_left_down = left_down;
            return;
        }
        // BPM 表示（Issue #35）: クリックで入力モードに入る。
        let bpm_r = bpm_rect(model.history.project().tracks.len());
        if lx >= bpm_r.x && lx < bpm_r.x + bpm_r.w && ly >= bpm_r.y && ly < bpm_r.y + bpm_r.h {
            model.bpm_edit = Some(edit::BpmEdit::new(model.history.project().bpm));
            println!("bpm edit started");
            model.prev_left_down = left_down;
            return;
        }
        // 色選択パネル（Issue #58）: トランスポートの下のパネル。
        // 他の領域とは重ならない。
        if let Some((target, color)) =
            palette::hit_palette(model.history.project().tracks.len(), lx, ly)
        {
            apply_palette_click(model, target, color);
            model.prev_left_down = left_down;
            return;
        }
        // 音色パネル（Issue #73）: 色パネルの下のパネル。
        // ダブルクリックで改名セッションに入る（Issue #76）。
        let tones_snapshot = model.history.project().tones.clone();
        if let Some(hit) = tones::hit_tones(
            model.history.project().tracks.len(),
            &tones_snapshot,
            lx,
            ly,
            win.w() - 16.0,
        ) {
            if let tones::ToneHit::Chip(index) = hit {
                let now_ms = (app.time * 1_000.0) as u64;
                if model.tone_clicks.register(now_ms, lx, ly) {
                    if let Some(tone) = tones_snapshot.get(index) {
                        model.tone_rename =
                            Some(tones::ToneRenameSession::begin(tone.id, &tone.name));
                        println!("tone rename begin: tone {}", tone.id);
                    }
                    model.prev_left_down = left_down;
                    return;
                }
            }
            apply_tone_click(model, hit);
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
        // ループ範囲の指定（Issue #40）: ルーラー上のドラッグで範囲を
        // 作る。クリップのドラッグとは領域が重ならない。
        if ly < timeline::RULER_HEIGHT_PX && lx >= timeline::HEADER_WIDTH_PX {
            model.loop_drag_anchor_ms = Some(pointer_ms);
            model.loop_drag_pointer_ms = pointer_ms;
            model.prev_left_down = left_down;
            return;
        }
        let target = edit::hit_test(model.history.project(), win.w(), &model.view_scale, lx, ly);
        // ピアノロールを開く導線（Issue #69）: クリップの
        // ダブルクリックで、そのクリップのロールを開く。
        // 対象の条件は roll_openable を参照。
        let target_clip = match &target {
            edit::HitTarget::ClipBody { clip_id }
            | edit::HitTarget::ClipLeftEdge { clip_id }
            | edit::HitTarget::ClipRightEdge { clip_id }
            | edit::HitTarget::ClipFadeInCorner { clip_id }
            | edit::HitTarget::ClipFadeOutCorner { clip_id } => Some(*clip_id),
            _ => None,
        };
        if let Some(clip_id) = target_clip {
            let now_ms = (app.time * 1_000.0) as u64;
            if model.roll_clicks.register(now_ms, lx, ly)
                && roll_openable(model.history.project(), clip_id)
            {
                let (_, clip) = model
                    .history
                    .project()
                    .clip(clip_id)
                    .expect("checked by roll_openable");
                model.piano_roll = Some(pianoroll::PianoRoll::open(clip));
                println!("piano roll opened: clip {clip_id}");
                model.prev_left_down = left_down;
                return;
            }
        }
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
            // スナップ（Issue #37）: しきい値は画面の 8px を ms に換算し、
            // Alt 押下中は無効化する。
            let threshold_ms =
                (edit::SNAP_THRESHOLD_PX / model.view_scale.px_per_ms(win.w())).round() as i64;
            let ctx = edit::SnapContext::from_project(
                model.history.project(),
                drag.clip_id,
                model.playhead_ms,
                threshold_ms,
            );
            let alt_down = app.keys.down.contains(&Key::LAlt) || app.keys.down.contains(&Key::RAlt);
            let cmd = drag.snapped_command_for(pointer_ms, &ctx, !alt_down);
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

    // ループ範囲ドラッグの追随と確定（Issue #40）。
    if let Some(anchor) = model.loop_drag_anchor_ms {
        if left_down {
            model.loop_drag_pointer_ms = pointer_ms;
        } else {
            model.loop_drag_anchor_ms = None;
            if let Some((start, end)) = timeline::loop_range_from_drag(anchor, pointer_ms) {
                model.loop_range = Some((start, end));
                model.loop_on = true;
                println!("loop range: {start}..{end}");
            }
        }
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
/// ピアノロールの描画（Issue #69）。テーマ（ADR-0012）に従い、
/// ノートはティール、選択中と発音中（現在状態）だけホットピンクで
/// 示す。鍵盤列には C の行に音名を出す。
fn draw_piano_roll(draw: &Draw, win: Rect, model: &Model, roll: &pianoroll::PianoRoll) {
    let project = model.history.project();
    let Some((track, clip)) = project.clip(roll.clip_id) else {
        return;
    };
    let grid_rect = pianoroll::grid(win.w(), win.h());
    let row_h = roll.row_h(win.h());

    // 見出し帯: ラベル＋クリップ名と楽器、右に操作ヒント。
    // タイトルはラベルとヒントの間の帯の中央に置く（Issue #74:
    // 以前の left_justify 配置ではタイトルがラベルに重なった）。
    draw_panel_label(
        draw,
        win,
        theme::PIANO_ROLL_LABEL,
        pianoroll::HEADER_LABEL_CENTER_X,
        timeline::RULER_HEIGHT_PX + pianoroll::HEADER_HEIGHT_PX / 2.0,
    );
    let effective = project.effective_instrument(track);
    let instrument = match &effective {
        Some(studio_core::Instrument::Sampler { source, root_note }) => {
            let base = source.rsplit('/').next().unwrap_or(source);
            format!(
                "instrument: {base} root {}",
                pianoroll::pitch_name(*root_note as i32)
            )
        }
        Some(studio_core::Instrument::Plugin {
            plugin_path,
            preset_path,
            ..
        }) => {
            let base = plugin_path.rsplit('/').next().unwrap_or(plugin_path);
            match preset_path {
                Some(preset) => {
                    let pbase = preset.rsplit('/').next().unwrap_or(preset);
                    format!("instrument: {base} preset {pbase}")
                }
                None => format!("instrument: {base}"),
            }
        }
        None => "instrument: (none)".to_string(),
    };
    // 音色が割り当てられていれば音色名を前置する（Issue #73）。
    let instrument = match track.tone_id.and_then(|id| project.tone(id)) {
        Some(tone) => format!("tone: {} / {}", tone.name, instrument),
        None => instrument,
    };
    let (r, g, b) = theme::TEXT;
    // タイトルは帯幅に収まるよう切り捨てる（Issue #77: 長い
    // クリップ名＋音色名＋プラグイン名では中央配置でも両端が
    // ラベル／ヒントに重なっていた）。draw.text は既定で
    // 折り返すため、タイトルとヒントは no_line_wrap で1行を
    // 保証する（折り返すと帯の下へはみ出す。実アプリ駆動で確認）。
    let (band_left, band_right) = pianoroll::header_title_band(win.w());
    let title = pianoroll::ellipsize_title(
        &format!("{} — {}", clip.name, instrument),
        band_right - band_left,
    );
    draw.text(&title)
        .x_y(
            pianoroll::header_title_center_x(win.w()) - win.w() / 2.0,
            win.h() / 2.0 - (timeline::RULER_HEIGHT_PX + pianoroll::HEADER_HEIGHT_PX / 2.0),
        )
        .font_size(pianoroll::HEADER_TITLE_FONT_SIZE as u32)
        .no_line_wrap()
        .color(rgb(r, g, b));
    let (r, g, b) = theme::TEXT_SUB;
    draw.text(pianoroll::HEADER_HINT_TEXT)
        .x_y(
            win.w() / 2.0 - pianoroll::HEADER_HINT_CENTER_FROM_RIGHT,
            win.h() / 2.0 - (timeline::RULER_HEIGHT_PX + pianoroll::HEADER_HEIGHT_PX / 2.0),
        )
        .font_size(pianoroll::HEADER_HINT_FONT_SIZE as u32)
        .no_line_wrap()
        .color(rgb(r, g, b));

    // 行（鍵盤列＋グリッド）: 黒鍵の行は一段沈んだ面にする。
    for row in 0..pianoroll::ROWS {
        let pitch = roll.top_pitch - row;
        let y = grid_rect.y + row as f32 * row_h;
        let black = pianoroll::is_black_key(pitch);
        if black {
            let (cx, cy) = rect_center(win, grid_rect.x, y, grid_rect.w, row_h);
            let (r, g, b) = theme::PANEL_SUNK;
            draw.rect()
                .x_y(cx, cy)
                .w_h(grid_rect.w, row_h)
                .color(rgb(r, g, b));
        }
        // 鍵盤セル
        let (kx, ky) = rect_center(win, 0.0, y, pianoroll::KEY_WIDTH_PX, row_h);
        let (r, g, b) = if black { theme::TEXT } else { theme::PANEL };
        draw.rect()
            .x_y(kx, ky)
            .w_h(pianoroll::KEY_WIDTH_PX, row_h)
            .color(rgb(r, g, b));
        if pitch.rem_euclid(12) == 0 {
            let (r, g, b) = theme::TEXT_SUB;
            draw.text(&pianoroll::pitch_name(pitch))
                .x_y(kx, ky)
                .font_size(10)
                .color(rgb(r, g, b));
        }
        draw_hline(draw, win, y + row_h - 1.0, theme::LINE);
    }
    draw_hline(draw, win, grid_rect.y - 1.0, theme::LINE);

    // 拍の縦線（プロジェクトの BPM 基準）。
    let beat_ms = (60_000.0 / project.bpm.max(1.0)) as i64;
    if beat_ms > 0 {
        let (r, g, b) = theme::LINE;
        let mut ms = 0;
        while ms <= clip.duration_ms {
            let x = roll.x_for_ms(win.w(), clip.duration_ms, ms) - win.w() / 2.0;
            draw.line()
                .start(pt2(x, win.h() / 2.0 - grid_rect.y))
                .end(pt2(x, win.h() / 2.0 - (grid_rect.y + grid_rect.h)))
                .weight(1.0)
                .color(rgb(r, g, b));
            ms += beat_ms;
        }
    }

    // ノート: 通常はティール、選択中と発音中はホットピンク。
    for (index, note) in clip.notes.iter().enumerate() {
        let rect = roll.note_rect(win.w(), win.h(), clip, note);
        let sounding = {
            let start = clip.start_ms + note.start_ms;
            model.playhead_ms >= start && model.playhead_ms < start + note.duration_ms
        };
        let face = if sounding || roll.selected == Some(index) {
            theme::ACCENT
        } else {
            theme::TEAL
        };
        let chip = mixer::Rect {
            x: rect.x + 1.0,
            y: rect.y + 1.5,
            w: (rect.w - 2.0).max(2.0),
            h: (rect.h - 3.0).max(2.0),
        };
        draw_rounded_rect(draw, win, chip, 2.5, face);
    }

    // 再生ヘッド（クリップの区間内だけ、クリップ相対で引く）。
    let rel = model.playhead_ms - clip.start_ms;
    if rel >= 0 && rel <= clip.duration_ms {
        let x = roll.x_for_ms(win.w(), clip.duration_ms, rel) - win.w() / 2.0;
        let (r, g, b) = theme::TEXT;
        draw.line()
            .start(pt2(x, win.h() / 2.0 - grid_rect.y))
            .end(pt2(x, win.h() / 2.0 - (grid_rect.y + grid_rect.h)))
            .weight(2.0)
            .color(rgb(r, g, b));
    }
}

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

/// トランスポートパネルの上端 y（Issue #27 のレイアウト）。
fn transport_top(track_count: usize) -> f32 {
    mixer::mixer_top_y(track_count) + track_count as f32 * mixer::STRIP_HEIGHT_PX + 28.0
}

/// BPM 表示の矩形（Issue #35）。トランスポート右側の大きな数値表示で、
/// クリックで入力モードに入る。
fn bpm_rect(track_count: usize) -> mixer::Rect {
    mixer::Rect {
        x: 695.0,
        y: transport_top(track_count) + 6.0,
        w: 150.0,
        h: 36.0,
    }
}

/// メトロノームトグルの矩形（Issue #36）。トランスポート左下のチップ。
fn metronome_rect(track_count: usize) -> mixer::Rect {
    mixer::Rect {
        x: 36.0,
        y: transport_top(track_count) + 38.0,
        w: 180.0,
        h: 16.0,
    }
}

/// ループトグルのチップ（Issue #40）。メトロノームチップの右隣。
fn loop_rect(track_count: usize) -> mixer::Rect {
    mixer::Rect {
        x: 228.0,
        y: transport_top(track_count) + 38.0,
        w: 180.0,
        h: 16.0,
    }
}

/// WAV 書き出しボタンのチップ（Issue #41）。ループチップの右隣。
/// 書き出し先はプロジェクトと同じ場所の <プロジェクト名>.wav
/// （初期値。ファイルダイアログは持たない）。
fn export_rect(track_count: usize) -> mixer::Rect {
    mixer::Rect {
        x: 420.0,
        y: transport_top(track_count) + 38.0,
        w: 180.0,
        h: 16.0,
    }
}

fn view(app: &App, model: &Model, frame: Frame) {
    let draw = app.draw();
    let win = app.window_rect();
    let (bg_r, bg_g, bg_b) = theme::BACKGROUND;
    draw.background().color(rgb(bg_r, bg_g, bg_b));

    // ピアノロール（Issue #69）が開いている間は画面モードを
    // 切り替える（タイムライン以下のパネルは描かない）。
    if let Some(roll) = &model.piano_roll {
        draw_piano_roll(&draw, win, model, roll);
        draw.to_frame(app, &frame).unwrap();
        return;
    }

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
    for sec in
        (model.view_scale.offset_ms / 1_000).max(0)..=(model.view_scale.end_ms(win.w()) / 1_000 + 1)
    {
        let x = timeline::time_to_x(sec * 1_000, win.w(), &model.view_scale);
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
    // ループ範囲（Issue #40）: ルーラー上に淡いピンクの帯を重ねる。
    // ドラッグ中は仮の範囲も同じ見た目で出す。
    {
        let shown = match (model.loop_drag_anchor_ms, model.loop_range) {
            (Some(anchor), _) => Some((
                anchor.min(model.loop_drag_pointer_ms).max(0),
                anchor.max(model.loop_drag_pointer_ms),
            )),
            (None, range) => range,
        };
        if let Some((start, end)) = shown {
            let x0 = timeline::time_to_x(start, win.w(), &model.view_scale);
            let x1 = timeline::time_to_x(end, win.w(), &model.view_scale);
            let (cx, cy) = rect_center(win, x0, 0.0, (x1 - x0).max(1.0), timeline::RULER_HEIGHT_PX);
            draw.rect()
                .x_y(cx, cy)
                .w_h((x1 - x0).max(1.0), timeline::RULER_HEIGHT_PX)
                .color(rgb(
                    theme::ACCENT_SOFT.0,
                    theme::ACCENT_SOFT.1,
                    theme::ACCENT_SOFT.2,
                ));
            draw_hline(&draw, win, timeline::RULER_HEIGHT_PX - 2.0, theme::ACCENT);
        }
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
    for rect in timeline::layout_clips(model.history.project(), win.w(), &model.view_scale) {
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
                        let within_ms = (col as f32 / rect.w) * clip.duration_ms as f32;
                        let frac =
                            timeline::waveform_source_frac(clip.offset_ms, within_ms, src_ms);
                        let idx = (frac * (peaks.len() - 1) as f32) as usize;
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
            // フェードの傾斜（Issue #34）: フェードインは左下→上、
            // フェードアウトは上→右下の直線で包絡を示す。
            let ppm = model.view_scale.px_per_ms(win.w());
            let face_x0 = rect.x + 1.5;
            let face_x1 = rect.x + rect.w - 1.5;
            let face_top = inner_y + 1.5;
            let face_bottom = inner_y + inner_h - 1.5;
            let to_nannou = |sx: f32, sy: f32| pt2(sx - win.w() / 2.0, win.h() / 2.0 - sy);
            let line_color = rgb(theme::TEXT.0, theme::TEXT.1, theme::TEXT.2);
            if clip.fade_in_ms > 0 {
                draw.line()
                    .start(to_nannou(face_x0, face_bottom))
                    .end(to_nannou(face_x0 + clip.fade_in_ms as f32 * ppm, face_top))
                    .weight(2.0)
                    .color(line_color);
            }
            if clip.fade_out_ms > 0 {
                draw.line()
                    .start(to_nannou(face_x1 - clip.fade_out_ms as f32 * ppm, face_top))
                    .end(to_nannou(face_x1, face_bottom))
                    .weight(2.0)
                    .color(line_color);
            }
            draw.text(&clip.name).x_y(cx, cy).font_size(11).color(rgb(
                theme::TEXT.0,
                theme::TEXT.1,
                theme::TEXT.2,
            ));
        }
    }

    // 小節線と小節番号（Issue #35、4/4 固定）: BPM から小節頭を計算し、
    // ルーラーに番号、トラック領域に縦線を引く。トラック行とクリップの
    // 描画後に重ねる（先に引くと行の地色で上塗りされる）。
    {
        let bpm = model.history.project().bpm;
        let tracks_bottom = timeline::RULER_HEIGHT_PX
            + model.history.project().tracks.len() as f32 * timeline::TRACK_HEIGHT_PX;
        let (lr, lg, lb) = theme::LINE;
        let bar_ms = timeline::bar_duration_ms(bpm);
        let mut bar = if bar_ms > 0.0 {
            (model.view_scale.offset_ms as f64 / bar_ms).floor() as i64
        } else {
            0
        };
        loop {
            let start = timeline::bar_start_ms(bpm, bar);
            if start > model.view_scale.end_ms(win.w()) {
                break;
            }
            let x = timeline::time_to_x(start, win.w(), &model.view_scale);
            let nx = x - win.w() / 2.0;
            draw.line()
                .start(pt2(nx, win.h() / 2.0 - timeline::RULER_HEIGHT_PX))
                .end(pt2(nx, win.h() / 2.0 - tracks_bottom))
                .weight(1.0)
                .color(rgb(lr, lg, lb));
            let (r, g, b) = theme::TEXT;
            draw.text(&format!("{}", bar + 1))
                .x_y(nx + 4.0, win.h() / 2.0 - 6.0)
                .font_size(10)
                .color(rgb(r, g, b));
            bar += 1;
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
        // M/S ボタン（Issue #39）: ON のときだけ色面になる
        // （M はアクセント、S は副色）。OFF は沈み面＋補助文字。
        for (rect, label, active, active_face) in [
            (
                mixer::mute_rect(count, index),
                "M",
                track.muted,
                theme::ACCENT,
            ),
            (mixer::solo_rect(count, index), "S", track.solo, theme::TEAL),
        ] {
            let face = if active {
                active_face
            } else {
                theme::PANEL_SUNK
            };
            draw_rounded_rect(&draw, win, rect, 4.0, face);
            let (lx, ly) = rect_center(win, rect.x, rect.y, rect.w, rect.h);
            let tc = if active {
                theme::BACKGROUND
            } else {
                theme::TEXT_SUB
            };
            draw.text(label)
                .x_y(lx, ly)
                .font_size(11)
                .color(rgb(tc.0, tc.1, tc.2));
        }
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
        let top = transport_top(count);
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
            timeline::bar_at_ms(model.playhead_ms, model.history.project().bpm),
            timeline::beat_at_ms(model.playhead_ms, model.history.project().bpm)
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
            .x_y(895.0 - win.w() / 2.0, win.h() / 2.0 - (top + 40.0))
            .font_size(11)
            .color(rgb(theme::TEXT_SUB.0, theme::TEXT_SUB.1, theme::TEXT_SUB.2));
        draw.text(&format!(
            "{:.1} / {:.1} s",
            model.playhead_ms as f32 / 1000.0,
            end_ms as f32 / 1000.0
        ))
        .x_y(895.0 - win.w() / 2.0, win.h() / 2.0 - (top + 24.0))
        .font_size(11)
        .color(rgb(theme::TEXT_SUB.0, theme::TEXT_SUB.1, theme::TEXT_SUB.2));
        // BPM の大きな表示（Issue #35）。入力中はバッファと下地を出す。
        let bpm_r = bpm_rect(count);
        if model.bpm_edit.is_some() {
            draw_rounded_rect(&draw, win, bpm_r, 4.0, theme::PANEL_SUNK);
        }
        let (bcx, bcy) = rect_center(win, bpm_r.x, bpm_r.y, bpm_r.w, bpm_r.h);
        let bpm_text = match &model.bpm_edit {
            Some(session) => format!("{} BPM", session.buffer()),
            None => {
                let bpm = model.history.project().bpm;
                if bpm.fract() == 0.0 {
                    format!("{} BPM", bpm as i64)
                } else {
                    format!("{bpm:.1} BPM")
                }
            }
        };
        draw.text(&bpm_text)
            .x_y(bcx, bcy)
            .font_size(theme::VALUE_FONT_SIZE)
            .color(rgb(theme::TEXT.0, theme::TEXT.1, theme::TEXT.2));
        // メトロノームトグル（Issue #36）: ON はピンクのチップ。
        let m_r = metronome_rect(count);
        let (m_face, m_text_color) = if model.metronome_on {
            (theme::ACCENT, theme::PANEL)
        } else {
            (theme::PANEL_SUNK, theme::TEXT_SUB)
        };
        draw_rounded_rect(&draw, win, m_r, 4.0, m_face);
        let (mcx, mcy) = rect_center(win, m_r.x, m_r.y, m_r.w, m_r.h);
        draw.text(if model.metronome_on {
            "METRONOME ON"
        } else {
            "METRONOME OFF"
        })
        .x_y(mcx, mcy)
        .font_size(10)
        .color(rgb(m_text_color.0, m_text_color.1, m_text_color.2));
        // ループトグル（Issue #40）: ON はピンクのチップ。
        let l_r = loop_rect(count);
        let (l_face, l_text_color) = if model.loop_on {
            (theme::ACCENT, theme::PANEL)
        } else {
            (theme::PANEL_SUNK, theme::TEXT_SUB)
        };
        draw_rounded_rect(&draw, win, l_r, 4.0, l_face);
        let (lcx, lcy) = rect_center(win, l_r.x, l_r.y, l_r.w, l_r.h);
        draw.text(if model.loop_on { "LOOP ON" } else { "LOOP OFF" })
            .x_y(lcx, lcy)
            .font_size(10)
            .color(rgb(l_text_color.0, l_text_color.1, l_text_color.2));
        // WAV 書き出しボタン（Issue #41）: 完了後は状態を表示する。
        let e_r = export_rect(count);
        draw_rounded_rect(&draw, win, e_r, 4.0, theme::PANEL_SUNK);
        let (ecx, ecy) = rect_center(win, e_r.x, e_r.y, e_r.w, e_r.h);
        draw.text(model.export_status.as_deref().unwrap_or("EXPORT WAV"))
            .x_y(ecx, ecy)
            .font_size(10)
            .color(rgb(theme::TEXT_SUB.0, theme::TEXT_SUB.1, theme::TEXT_SUB.2));
        // プラグインエラー（Issue #72）: 失敗したトラックがある間、
        // チップ行の右にピンクで明示する（無言で無音にしない）。
        // 全文は stdout に出ているため、画面では1行に切り詰める。
        if let Some(first) = model.plugin_errors.first() {
            let mut text: String = first.chars().take(40).collect();
            if first.chars().count() > 40 || model.plugin_errors.len() > 1 {
                text.push_str(" …");
            }
            if model.plugin_errors.len() > 1 {
                text = format!("{}（他{}件）", text, model.plugin_errors.len() - 1);
            }
            draw.text(&text)
                .x_y(612.0 - win.w() / 2.0, win.h() / 2.0 - (top + 46.0))
                .left_justify()
                .font_size(10)
                .color(rgb(theme::ACCENT.0, theme::ACCENT.1, theme::ACCENT.2));
        }
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

    // 音色パネル（Issue #73、ADR-0018）: 色パネルの下。チップは
    // プロジェクトの音色一覧で、選択中トラックに割り当て中のものは
    // アクセント色で灯す。末尾は作成／削除ボタン。
    {
        let project = model.history.project();
        let count = project.tracks.len();
        let top = tones::panel_top(count);
        draw_rounded_rect(
            &draw,
            win,
            mixer::Rect {
                x: 8.0,
                y: top - 8.0,
                w: win.w() - 16.0,
                h: tones::panel_height(&project.tones, win.w() - 16.0),
            },
            theme::PANEL_CORNER_RADIUS,
            theme::PANEL,
        );
        draw_panel_label(&draw, win, theme::TONES_LABEL, 40.0, top);
        let assigned = model
            .selected_track
            .and_then(|id| project.track(id))
            .and_then(|t| t.tone_id);
        let (lx, ly) = rect_center(
            win,
            8.0,
            top + tones::ROWS_TOP_OFFSET_PX,
            tones::CHIP_X_PX - 8.0,
            tones::ROW_H_PX,
        );
        draw.text("TRACK").x_y(lx, ly).font_size(10).color(rgb(
            theme::TEXT_SUB.0,
            theme::TEXT_SUB.1,
            theme::TEXT_SUB.2,
        ));
        for (index, tone) in project.tones.iter().enumerate() {
            let rect = tones::chip_rect(top, &project.tones, index, win.w() - 16.0);
            let is_assigned = assigned == Some(tone.id);
            let (face, text_color) = if is_assigned {
                (theme::ACCENT, theme::PANEL)
            } else {
                (theme::PANEL_SUNK, theme::TEXT)
            };
            draw_rounded_rect(&draw, win, rect, 4.0, face);
            let (cx, cy) = rect_center(win, rect.x, rect.y, rect.w, rect.h);
            // 改名編集中のチップは編集中の文字列＋キャレットを出す
            // （Issue #76、トラック改名と同じ流儀）。
            let renaming_this = model.tone_rename.as_ref().filter(|s| s.tone_id == tone.id);
            let label: String = match renaming_this {
                Some(session) => format!("{}_", session.text),
                None => tone.name.chars().take(18).collect(),
            };
            draw.text(&label).x_y(cx, cy).font_size(10).color(rgb(
                text_color.0,
                text_color.1,
                text_color.2,
            ));
        }
        let add = tones::add_rect(top, &project.tones, win.w() - 16.0);
        draw_rounded_rect(&draw, win, add, 4.0, theme::PANEL_SUNK);
        let (cx, cy) = rect_center(win, add.x, add.y, add.w, add.h);
        draw.text("+ TONE FROM TRACK")
            .x_y(cx, cy)
            .font_size(10)
            .color(rgb(theme::TEXT.0, theme::TEXT.1, theme::TEXT.2));
        let del = tones::delete_rect(top, &project.tones, win.w() - 16.0);
        draw_rounded_rect(&draw, win, del, 4.0, theme::PANEL_SUNK);
        let (cx, cy) = rect_center(win, del.x, del.y, del.w, del.h);
        draw.text("DELETE ASSIGNED")
            .x_y(cx, cy)
            .font_size(10)
            .color(rgb(theme::TEXT.0, theme::TEXT.1, theme::TEXT.2));
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

    // スクロールバー（Issue #87）: タイムライン領域の下端の帯に、
    // 現在の表示範囲とプロジェクト全体の割合でつまみを描く。
    // ヘッダー列（トラック追加エリア）には被せない。色は DAW
    // テーマ（帯＝LINE、つまみ＝TEXT_SUB、ドラッグ中＝ACCENT）。
    {
        let track_count = model.history.project().tracks.len();
        let y = timeline::scrollbar_y(track_count);
        let end = timeline::project_end_ms(model.history.project());
        let (bx, by) = rect_center(
            win,
            timeline::HEADER_WIDTH_PX,
            y,
            win.w() - timeline::HEADER_WIDTH_PX,
            timeline::SCROLLBAR_HEIGHT_PX,
        );
        draw.rect()
            .x_y(bx, by)
            .w_h(
                win.w() - timeline::HEADER_WIDTH_PX,
                timeline::SCROLLBAR_HEIGHT_PX,
            )
            .color(rgb(theme::LINE.0, theme::LINE.1, theme::LINE.2));
        let (thumb_x, thumb_w) = model.view_scale.scrollbar_thumb(end, win.w());
        let (tx, ty) = rect_center(
            win,
            thumb_x,
            y + 2.0,
            thumb_w,
            timeline::SCROLLBAR_HEIGHT_PX - 4.0,
        );
        let thumb = if model.scrollbar_drag.is_some() {
            theme::ACCENT
        } else {
            theme::TEXT_SUB
        };
        draw.rect()
            .x_y(tx, ty)
            .w_h(thumb_w, timeline::SCROLLBAR_HEIGHT_PX - 4.0)
            .color(rgb(thumb.0, thumb.1, thumb.2));
    }

    // 再生ヘッド（timeline.playhead）: 黒の細線＋現在の小節チップ
    // （Issue #24、デザイン文書「ライブ感の要素」）。
    let px = timeline::playhead_x(model.playhead_ms, win.w(), &model.view_scale) - win.w() / 2.0;
    draw.line()
        .start(pt2(px, win.h() / 2.0))
        .end(pt2(px, -win.h() / 2.0))
        .weight(1.5)
        .color(rgb(theme::TEXT.0, theme::TEXT.1, theme::TEXT.2));
    let chip_w = 48.0;
    let chip_h = 18.0;
    let chip_x = timeline::playhead_x(model.playhead_ms, win.w(), &model.view_scale)
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
    draw.text(&format!(
        "BAR {}",
        timeline::bar_at_ms(model.playhead_ms, model.history.project().bpm)
    ))
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
        // handle_history_keys が使う純粋関数そのもので検証する:
        // Undo はジェスチャを確定させてから消費する
        let mut mixer_before: Option<Project> = None;
        assert!(edit::apply_history_key(
            &mut history,
            edit::HistoryOp::Undo,
            &mut [&mut before, &mut mixer_before],
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
        let mut project = Project::new("gesture redo test");
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
        // スナップショットは保持され、途中状態もそのまま
        assert!(before.is_some());
        assert_eq!(history.project().tracks[0].clips[0].start_ms, 900);
        assert!(!history.can_undo());
        // ジェスチャ終了時に記録され、undo で開始位置へ戻れる
        finish_gesture_snapshot(&mut history, &mut before);
        assert!(history.can_undo());
        assert!(history.undo());
        assert_eq!(history.project().tracks[0].clips[0].start_ms, 0);
    }

    #[test]
    fn failed_undo_during_unchanged_gesture_keeps_the_gesture_armed() {
        // Issue #31 レビュー round 4 対応: 未変化ジェスチャの途中で
        // Undo が空振り（履歴が空）しても、スナップショットは消費
        // されず、その後の動きが終了時に記録される。
        let mut project = Project::new("gesture failed undo test");
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
        assert_eq!(history.project().tracks[0].clips[0].start_ms, 0);
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
