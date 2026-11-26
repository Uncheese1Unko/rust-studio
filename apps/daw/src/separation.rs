//! 音声分離（Issue #99、ADR-0024）: Demucs を別プロセスで実行し、
//! ステム WAV をプロジェクトへ原子的に取り込む。
//!
//! 実行環境の解決・生成物の置き場・適用の原子性は ADR-0024 の骨格に
//! 従う。重い処理（プロセス実行、音声デコード）はアプリ側に置き、
//! 状態変更はコマンド層経由のみ（architecture ルール）。適用は
//! 複製の上でコマンド列を組み立ててから
//! [`History::commit_replacement`] で一度に差し替えるため、失敗して
//! も部分適用が残らず、Undo 1回で分離前に戻れる（MIDI 取り込みと
//! 同じ性質）。

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcCommand, Stdio};

use studio_core::{apply, Command, History, TrackKind};

/// 分離モデル（ADR-0024: htdemucs、4ステム）。
pub const MODEL_NAME: &str = "htdemucs";
/// htdemucs のステム名。プロジェクトへの追加順もこの順に固定する。
pub const STEM_NAMES: [&str; 4] = ["vocals", "drums", "bass", "other"];
/// Demucs 用 Python を指す環境変数（ADR-0024 の解決順1）。
pub const PYTHON_ENV_VAR: &str = "RUST_STUDIO_DEMUCS_PYTHON";

/// 分離の失敗。利用者へ見える形で返す（GUI はステータス表示、
/// CLI は終了コード＋メッセージ）。
#[derive(Debug, Clone, PartialEq)]
pub enum SeparationError {
    /// venv の Python が見つからない、または demucs を import
    /// できない（未導入）。黙って波形配置だけに倒さないための
    /// 明示エラー（ADR-0024）。
    NotInstalled(String),
    /// Demucs プロセスの起動失敗・異常終了・出力不足。
    Failed(String),
    /// コマンド適用の失敗（入力は検証済みのため通常は起きない）。
    Command(String),
}

impl fmt::Display for SeparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SeparationError::NotInstalled(msg) => {
                write!(f, "demucs is not installed: {msg}")
            }
            SeparationError::Failed(msg) => write!(f, "demucs separation failed: {msg}"),
            SeparationError::Command(msg) => {
                write!(f, "command failed during separation import: {msg}")
            }
        }
    }
}

impl std::error::Error for SeparationError {}

/// FNV-1a 64bit（main.rs のファイル変更検出と同じ実装。パスの
/// ハッシュで生成物ディレクトリを決定的に分けるために使う）。
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Demucs 用 Python の解決（ADR-0024）。`env_val` は環境変数
/// [`PYTHON_ENV_VAR`] の値、`home` はホームディレクトリ。
/// どちらでも実在する実行ファイルに解決できなければ
/// [`SeparationError::NotInstalled`]。
pub fn resolve_python(
    env_val: Option<&str>,
    home: Option<&Path>,
) -> Result<PathBuf, SeparationError> {
    if let Some(value) = env_val {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Ok(path);
        }
        return Err(SeparationError::NotInstalled(format!(
            "{PYTHON_ENV_VAR} points to {value}, which is not a file; \
             install demucs into a venv or fix the variable"
        )));
    }
    if let Some(home) = home {
        let path = home.join("demucs-venv").join("bin").join("python");
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(SeparationError::NotInstalled(
        "no demucs venv found (looked at ~/demucs-venv/bin/python); \
         create the venv and install demucs (CPU torch), or set RUST_STUDIO_DEMUCS_PYTHON"
            .to_string(),
    ))
}

/// 実行環境から Python を解決する（[`resolve_python`] の実環境版）。
pub fn resolve_python_default() -> Result<PathBuf, SeparationError> {
    let env_val = std::env::var(PYTHON_ENV_VAR).ok();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    resolve_python(env_val.as_deref(), home.as_deref())
}

/// 指定した Python で demucs を import できるか確かめる。
/// できなければ [`SeparationError::NotInstalled`]。
pub fn check_demucs(python: &Path) -> Result<(), SeparationError> {
    let output = ProcCommand::new(python)
        .args(["-c", "import demucs"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| {
            SeparationError::NotInstalled(format!("cannot run {}: {e}", python.display()))
        })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail: String = stderr
        .lines()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" / ");
    Err(SeparationError::NotInstalled(format!(
        "{} cannot import demucs ({tail})",
        python.display()
    )))
}

/// ステム生成物のルート（ADR-0024）。プロジェクトファイルがある
/// 場合はその同階層の `<プロジェクト名>.stems/`、無い場合
/// （GUI デモモード）は `media_dir/stems/`。
pub fn stems_root(project_file: Option<&Path>, media_dir: &Path) -> PathBuf {
    match project_file {
        Some(path) => {
            // Stem paths end up stored as clip `source` values in the
            // saved project, so the root must be absolute: a relative
            // project path (e.g. the CLI's `project.json`) would
            // otherwise store cwd-relative sources that stop resolving
            // as soon as the app runs from another directory
            // (review #99 round 1).
            let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
            let stem = abs
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "project".to_string());
            let parent = abs.parent().unwrap_or_else(|| Path::new(""));
            parent.join(format!("{stem}.stems"))
        }
        None => media_dir.join("stems"),
    }
}

/// 1入力分の出力先。`<入力ファイル名>-<入力パスのハッシュ>` で
/// 決定的に決まり、再実行時は同じ場所へ上書きされる。
pub fn separation_out_dir(root: &Path, input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "audio".to_string());
    let hash = fnv1a(input.to_string_lossy().as_bytes());
    root.join(format!("{stem}-{hash:016x}"))
}

/// Demucs の出力レイアウト（`<out>/htdemucs/<曲名>/<ステム>.wav`）
/// におけるステム WAV のパス。
pub fn stem_output_path(out_dir: &Path, input: &Path, stem: &str) -> PathBuf {
    let track = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "audio".to_string());
    out_dir
        .join(MODEL_NAME)
        .join(track)
        .join(format!("{stem}.wav"))
}

/// Demucs を別プロセスで実行し、4ステムの WAV パスを
/// [`STEM_NAMES`] の順で返す。初回はモデルのダウンロードを含む。
pub fn run_demucs(
    python: &Path,
    input: &Path,
    out_dir: &Path,
) -> Result<Vec<PathBuf>, SeparationError> {
    std::fs::create_dir_all(out_dir).map_err(|e| {
        SeparationError::Failed(format!(
            "cannot create output dir {}: {e}",
            out_dir.display()
        ))
    })?;
    let output = ProcCommand::new(python)
        .arg("-m")
        .arg("demucs")
        .arg("-n")
        .arg(MODEL_NAME)
        .arg("-d")
        .arg("cpu")
        .arg("-o")
        .arg(out_dir)
        .arg(input)
        // Demucs 4.1 はモデルの取得にまず HuggingFace を試し、
        // 到達できない環境では応答待ちで長時間止まることがある
        // （試験レポート issue-0099 の実測）。同じ htdemucs の
        // 重みは従来の配布経路（torch hub キャッシュ／
        // dl.fbaipublicfiles.com）からも得られるため、HF は
        // オフラインにして即座にそちらへ落とす。
        .env("HF_HUB_OFFLINE", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| SeparationError::Failed(format!("cannot run {}: {e}", python.display())))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = {
            let lines: Vec<&str> = stderr.lines().collect();
            let start = lines.len().saturating_sub(5);
            lines[start..].join(" / ")
        };
        return Err(SeparationError::Failed(format!(
            "process failed ({}): {tail}",
            output.status
        )));
    }
    let mut paths = Vec::new();
    for stem in STEM_NAMES {
        let path = stem_output_path(out_dir, input, stem);
        if !path.is_file() {
            return Err(SeparationError::Failed(format!(
                "expected stem output is missing: {}",
                path.display()
            )));
        }
        paths.push(path);
    }
    Ok(paths)
}

/// 分離パイプラインの成果物。プロジェクトへの適用
/// （[`apply_stems`]）の入力になる。
#[derive(Debug, Clone, PartialEq)]
pub struct SeparationOutcome {
    /// トラック名の接頭辞になる、入力ファイル名（拡張子なし）。
    pub base_name: String,
    pub stems: Vec<StemInput>,
}

/// WAV ファイルを分離し、ステムをデコードして長さまで用意する
/// （プロジェクトへの適用は含まない）。モデル取得を含む重い処理
/// なので、GUI ではワーカースレッドから呼ぶこと。
pub fn separate_file(
    python: &Path,
    project_file: Option<&Path>,
    media_dir: &Path,
    input: &Path,
) -> Result<SeparationOutcome, SeparationError> {
    check_demucs(python)?;
    let root = stems_root(project_file, media_dir);
    let out_dir = separation_out_dir(&root, input);
    let paths = run_demucs(python, input, &out_dir)?;
    let mut stems = Vec::new();
    for (stem, path) in STEM_NAMES.iter().zip(paths.iter()) {
        let decoded = crate::audio::decode_audio(path).map_err(|e| {
            SeparationError::Failed(format!(
                "cannot decode separated stem {}: {e}",
                path.display()
            ))
        })?;
        // メロディ抽出（#100）: 単旋律ピッチ追跡。音高が取れない
        // ステムはノート列が空になり、波形クリップのまま扱われる。
        let notes = crate::pitch::extract_notes(&decoded);
        stems.push(StemInput {
            stem: stem.to_string(),
            source: path.to_string_lossy().into_owned(),
            duration_ms: decoded.duration_ms(),
            notes,
        });
    }
    let base_name = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "audio".to_string());
    Ok(SeparationOutcome { base_name, stems })
}

/// プロジェクトへ追加するステム1つ分（クリップの材料）。
#[derive(Debug, Clone, PartialEq)]
pub struct StemInput {
    /// ステム名（vocals 等）。
    pub stem: String,
    /// ステム WAV へのパス（波形クリップの `source` になる）。
    pub source: String,
    /// ステム WAV の長さ（デコードして得る）。
    pub duration_ms: i64,
    /// ステム音声から抽出したノート列（#100）。空なら音高が
    /// 取れなかったステムとして波形クリップのまま置く。
    pub notes: Vec<crate::pitch::ExtractedNote>,
}

/// 取り込みの結果サマリ（入口が表示に使う）。
#[derive(Debug, Clone, PartialEq)]
pub struct SeparationSummary {
    pub tracks_added: usize,
}

/// ステム列をプロジェクトへ原子的に追加する。各ステムは新規
/// トラック＋クリップ（開始 0、ステム全体）になり、音高が取れた
/// ステムはノートクリップ、取れなかったステムは波形クリップに
/// なる。失敗時はプロジェクトも履歴も変わらない。成功時は
/// Undo 1回で分離前に戻れる。分離前の既存トラック／クリップには
/// 触れない。
pub fn apply_stems(
    history: &mut History,
    base_name: &str,
    stems: &[StemInput],
) -> Result<SeparationSummary, SeparationError> {
    // 複製の上でコマンド列を最後まで適用してから一度に差し替える
    // ため、失敗しても実プロジェクトに部分適用が残らない
    // （MIDI 取り込みと同じ方式）。
    let mut built = history.project().clone();
    for stem in stems {
        let track_id = apply(
            &mut built,
            &Command::AddTrack {
                name: format!("{base_name} / {}", stem.stem),
                kind: TrackKind::Audio,
            },
        )
        .map_err(|e| SeparationError::Command(e.to_string()))?
        .ok_or_else(|| SeparationError::Command("add_track returned no id".to_string()))?;
        // 音高が取れたステムはノートクリップ（source は空、ノートは
        // AddNote コマンド経由でモデルの不変条件を通す）、取れな
        // かったステムは波形クリップにする（ADR-0024 の最終形）。
        let clip_id = apply(
            &mut built,
            &Command::AddClip {
                track_id,
                name: stem.stem.clone(),
                start_ms: 0,
                duration_ms: stem.duration_ms,
                source: if stem.notes.is_empty() {
                    stem.source.clone()
                } else {
                    String::new()
                },
            },
        )
        .map_err(|e| SeparationError::Command(e.to_string()))?
        .ok_or_else(|| SeparationError::Command("add_clip returned no id".to_string()))?;
        for note in &stem.notes {
            apply(
                &mut built,
                &Command::AddNote {
                    clip_id,
                    pitch: note.pitch,
                    start_ms: note.start_ms,
                    duration_ms: note.duration_ms,
                    velocity: note.velocity,
                },
            )
            .map_err(|e| SeparationError::Command(e.to_string()))?;
        }
    }
    history.commit_replacement(built);
    Ok(SeparationSummary {
        tracks_added: stems.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::Project;

    /// テスト専用の一時ディレクトリ（テスト名で分ける）。
    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("daw-separation-{}-{}", std::process::id(), tag));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"x").unwrap();
    }

    #[test]
    fn resolve_python_prefers_env_var() {
        let dir = temp_dir("env");
        let python = dir.join("custom-python");
        touch(&python);
        let resolved =
            resolve_python(Some(python.to_str().unwrap()), None).expect("env python resolves");
        assert_eq!(resolved, python);
    }

    #[test]
    fn resolve_python_env_var_pointing_nowhere_is_not_installed() {
        let err = resolve_python(Some("/nonexistent/demucs-python"), None)
            .expect_err("missing env python must be an explicit error");
        assert!(matches!(err, SeparationError::NotInstalled(_)), "{err}");
        assert!(
            err.to_string().contains("/nonexistent/demucs-python"),
            "{err}"
        );
    }

    #[test]
    fn resolve_python_falls_back_to_conventional_venv() {
        let home = temp_dir("home-with-venv");
        let python = home.join("demucs-venv/bin/python");
        touch(&python);
        let resolved = resolve_python(None, Some(&home)).expect("venv python resolves");
        assert_eq!(resolved, python);
    }

    #[test]
    fn resolve_python_without_venv_is_not_installed() {
        let home = temp_dir("home-without-venv");
        let err = resolve_python(None, Some(&home)).expect_err("no venv must be an explicit error");
        assert!(matches!(err, SeparationError::NotInstalled(_)), "{err}");
    }

    #[test]
    fn check_demucs_rejects_python_that_cannot_import() {
        // /bin/false は引数を無視して非 0 で終わる = import 失敗の再現。
        let err =
            check_demucs(Path::new("/bin/false")).expect_err("import failure must be NotInstalled");
        assert!(matches!(err, SeparationError::NotInstalled(_)), "{err}");
    }

    #[test]
    fn check_demucs_accepts_zero_exit() {
        // /bin/true は引数を無視して 0 で終わる = import 成功の再現。
        check_demucs(Path::new("/bin/true")).expect("zero exit means demucs is available");
    }

    #[test]
    fn stems_root_prefers_project_sibling_dir() {
        let root = stems_root(Some(Path::new("/work/session.json")), Path::new("/media"));
        assert_eq!(root, PathBuf::from("/work/session.stems"));
    }

    #[test]
    fn stems_root_demo_mode_uses_media_dir() {
        let root = stems_root(None, Path::new("/media"));
        assert_eq!(root, PathBuf::from("/media/stems"));
    }

    #[test]
    fn stems_root_absolutizes_a_relative_project_path() {
        // Review #99 round 1: stem `source` values are stored in the
        // saved project, so a relative project path must not leak a
        // cwd-relative stems root into them — from any other working
        // directory the stems would no longer resolve.
        let root = stems_root(Some(Path::new("song.json")), Path::new("/media"));
        assert!(root.is_absolute(), "{root:?}");
        assert!(root.ends_with("song.stems"), "{root:?}");
    }

    #[test]
    fn separation_out_dir_is_deterministic_and_path_sensitive() {
        let root = Path::new("/stems");
        let a1 = separation_out_dir(root, Path::new("/music/song.wav"));
        let a2 = separation_out_dir(root, Path::new("/music/song.wav"));
        let b = separation_out_dir(root, Path::new("/other/song.wav"));
        assert_eq!(a1, a2, "same input must map to the same dir");
        assert_ne!(a1, b, "different input paths must not collide");
        assert!(a1
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("song-"));
    }

    #[test]
    fn stem_output_path_follows_demucs_layout() {
        let path = stem_output_path(Path::new("/out"), Path::new("/music/song.wav"), "vocals");
        assert_eq!(path, PathBuf::from("/out/htdemucs/song/vocals.wav"));
    }

    #[test]
    fn run_demucs_with_missing_python_is_error_not_panic() {
        let dir = temp_dir("run-missing");
        let err = run_demucs(
            Path::new("/nonexistent/python"),
            Path::new("/music/song.wav"),
            &dir,
        )
        .expect_err("spawning a missing python must be an error");
        assert!(matches!(err, SeparationError::Failed(_)), "{err}");
    }

    #[test]
    fn separate_file_reports_missing_stem_output() {
        // /bin/true は「demucs の実行」を 0 終了で再現するがファイルを
        // 作らないため、出力不足として Failed になるはず。
        let dir = temp_dir("separate-true");
        let input = dir.join("song.wav");
        touch(&input);
        let err = separate_file(Path::new("/bin/true"), None, &dir.join("media"), &input)
            .expect_err("missing stem outputs must be an error");
        assert!(matches!(err, SeparationError::Failed(_)), "{err}");
        assert!(err.to_string().contains("missing"), "{err}");
    }

    fn stem(name: &str, duration_ms: i64) -> StemInput {
        StemInput {
            stem: name.to_string(),
            source: format!("/stems/song/{name}.wav"),
            duration_ms,
            notes: Vec::new(),
        }
    }

    #[test]
    fn apply_stems_adds_waveform_track_and_clip_per_stem() {
        let mut project = Project::new("t");
        // 既存トラック（元 WAV のつもり）は不変であること。
        studio_core::apply(
            &mut project,
            &Command::AddTrack {
                name: "orig".into(),
                kind: studio_core::TrackKind::Audio,
            },
        )
        .unwrap();
        let mut history = History::new(project);
        let before = history.project().dump_json();

        let stems: Vec<StemInput> = STEM_NAMES.iter().map(|s| stem(s, 2000)).collect();
        let summary = apply_stems(&mut history, "song", &stems).expect("apply succeeds");
        assert_eq!(summary.tracks_added, 4);
        let after = history.project();
        assert_eq!(after.tracks.len(), 5);
        for (i, name) in STEM_NAMES.iter().enumerate() {
            let track = &after.tracks[i + 1];
            assert_eq!(track.name, format!("song / {name}"));
            assert_eq!(track.clips.len(), 1);
            let clip = &track.clips[0];
            assert_eq!(clip.source, format!("/stems/song/{name}.wav"));
            assert_eq!(clip.start_ms, 0);
            assert_eq!(clip.duration_ms, 2000);
            assert!(clip.notes.is_empty(), "no notes extracted -> waveform clip");
        }
        // Undo 1回で分離前に戻れる。
        assert!(history.undo());
        assert_eq!(history.project().dump_json(), before);
    }

    #[test]
    fn apply_stems_with_notes_adds_note_clip() {
        use crate::pitch::ExtractedNote;
        let mut history = History::new(Project::new("t"));
        let mut pitched = stem("vocals", 2000);
        pitched.notes = vec![
            ExtractedNote {
                pitch: 69,
                start_ms: 0,
                duration_ms: 500,
                velocity: 90,
            },
            ExtractedNote {
                pitch: 72,
                start_ms: 500,
                duration_ms: 500,
                velocity: 80,
            },
        ];
        let stems = vec![pitched, stem("drums", 2000)];
        let summary = apply_stems(&mut history, "song", &stems).expect("apply succeeds");
        assert_eq!(summary.tracks_added, 2);
        let project = history.project();
        // 音高が取れたステム: ノートクリップ（source は空）。
        let vocals = &project.tracks[0];
        assert_eq!(vocals.clips.len(), 1);
        let clip = &vocals.clips[0];
        assert_eq!(clip.source, "");
        assert_eq!(clip.notes.len(), 2);
        assert_eq!(clip.notes[0].pitch, 69);
        assert_eq!(clip.notes[0].start_ms, 0);
        assert_eq!(clip.notes[0].duration_ms, 500);
        assert_eq!(clip.notes[0].velocity, 90);
        assert_eq!(clip.notes[1].pitch, 72);
        // 取れなかったステム: 波形クリップのまま。
        let drums = &project.tracks[1];
        assert_eq!(drums.clips[0].source, "/stems/song/drums.wav");
        assert!(drums.clips[0].notes.is_empty());
    }

    #[test]
    fn apply_stems_rejects_notes_outside_the_clip() {
        use crate::pitch::ExtractedNote;
        let mut history = History::new(Project::new("t"));
        let before = history.project().dump_json();
        let mut bad = stem("vocals", 1000);
        // クリップ範囲を超えるノートは AddNote の検証で落ち、
        // 取り込み全体が失敗して状態は変わらない。
        bad.notes = vec![ExtractedNote {
            pitch: 69,
            start_ms: 900,
            duration_ms: 500,
            velocity: 90,
        }];
        let err = apply_stems(&mut history, "song", &[bad])
            .expect_err("out-of-clip note must fail the whole import");
        assert!(matches!(err, SeparationError::Command(_)), "{err}");
        assert_eq!(history.project().dump_json(), before);
        assert!(!history.can_undo());
    }

    #[test]
    fn apply_stems_failure_leaves_project_and_history_untouched() {
        let mut history = History::new(Project::new("t"));
        let before = history.project().dump_json();
        // 長さ 0 のステムは AddClip の検証で落ちる。
        let stems = vec![stem("vocals", 2000), stem("drums", 0)];
        let err = apply_stems(&mut history, "song", &stems)
            .expect_err("invalid stem must fail the whole import");
        assert!(matches!(err, SeparationError::Command(_)), "{err}");
        assert_eq!(history.project().dump_json(), before);
        assert!(!history.can_undo(), "failed import must not add history");
    }
}
