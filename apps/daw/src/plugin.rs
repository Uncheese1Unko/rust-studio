//! プラグイン連携のクライアント側（Issue #72、ADR-0017）。
//!
//! CLAP プラグインの実レンダリングは別プロセスのヘルパー
//! （`plugin-host` バイナリ）に任せ、本体は JSON リクエストを
//! stdin で渡して raw PCM を受け取るだけにする。`render_mix` は
//! 純粋関数のままにしたいので、プラグイントラックの音声は
//! 再生・書き出しの前にここで事前レンダリングし、音声マップへ
//! `audio::plugin_track_key` のキーで渡す。
//!
//! プラグイン／プリセット／ヘルパーが無い場合は、そのトラックを
//! ミックスから落とすだけでなく `PluginTrackError` として明示的に
//! 返す（呼び出し側が画面に表示する。無言で無音にしない）。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use studio_core::{Instrument, PluginFormat, Project, TrackKind};

use crate::audio::{plugin_track_key, ClipAudio};

/// ヘルパーの応答待ち上限。長いプロジェクトのレンダリングでも
/// プラグインの暴走で本体が固まらないための制限。
const HELPER_TIMEOUT: Duration = Duration::from_secs(120);

/// ヘルパーへ渡すノート（絶対時刻、プロジェクト先頭基準）。
#[derive(Debug, Clone, PartialEq, Serialize)]
struct NoteJson {
    start_ms: f64,
    duration_ms: f64,
    pitch: u8,
    velocity: u8,
}

/// ヘルパーへのリクエスト（plugin-host バイナリのプロトコル）。
#[derive(Debug, Serialize)]
struct RequestJson {
    plugin_path: String,
    sample_rate: u32,
    duration_ms: f64,
    state_json: Option<String>,
    notes: Vec<NoteJson>,
    output_path: String,
}

/// ヘルパーのステータス応答。
#[derive(Debug, Deserialize)]
struct StatusJson {
    ok: bool,
    frames: Option<usize>,
    channels: Option<u16>,
    sample_rate: Option<u32>,
    error: Option<String>,
}

/// ノート列をヘルパー経由でレンダリングする。失敗は理由を文字列で
/// 返す（呼び出し側がトラック単位のエラーとして表示する）。
fn render_notes(
    helper: &Path,
    plugin_path: &str,
    preset_json: Option<String>,
    notes: &[NoteJson],
    duration_ms: i64,
    sample_rate: u32,
) -> Result<ClipAudio, String> {
    let output_path = std::env::temp_dir().join(format!(
        "rust-studio-plugin-{}-{}.pcm",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let request = RequestJson {
        plugin_path: plugin_path.to_string(),
        sample_rate,
        duration_ms: duration_ms as f64,
        state_json: preset_json,
        notes: notes.to_vec(),
        output_path: output_path.to_string_lossy().into_owned(),
    };
    let request_text = serde_json::to_string(&request)
        .map_err(|e| format!("プラグインリクエストを作れません: {e}"))?;

    let mut child = std::process::Command::new(helper)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| {
            format!(
                "プラグインヘルパーを起動できません ({}): {e}",
                helper.display()
            )
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        // ヘルパーが先に死んでいる場合の書き込み失敗は、終了後の
        // ステータスで判定するため、ここでは握りつぶす。
        let _ = stdin.write_all(request_text.as_bytes());
    }
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() > HELPER_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("プラグインヘルパーがタイムアウトしました".to_string());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(format!("プラグインヘルパーの状態を確認できません: {e}")),
        }
    };
    let mut stdout_text = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut stdout_text);
    }
    let parsed: Option<StatusJson> = stdout_text
        .lines()
        .next_back()
        .and_then(|line| serde_json::from_str(line).ok());
    let parsed = match parsed {
        Some(p) => p,
        None => {
            return Err(format!(
                "プラグインヘルパーが異常終了しました (status: {status})"
            ))
        }
    };
    if !parsed.ok {
        return Err(parsed
            .error
            .unwrap_or_else(|| "プラグインのレンダリングに失敗しました".to_string()));
    }
    let channels = parsed.channels.unwrap_or(2);
    let rate = parsed.sample_rate.unwrap_or(sample_rate);
    let bytes =
        std::fs::read(&output_path).map_err(|e| format!("プラグインの出力PCMを読めません: {e}"))?;
    let _ = std::fs::remove_file(&output_path);
    if bytes.len() % 4 != 0 {
        return Err("プラグインの出力PCMが壊れています".to_string());
    }
    let samples: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect();
    if let Some(frames) = parsed.frames {
        let expected = frames * channels as usize;
        if samples.len() != expected {
            return Err(format!(
                "プラグインの出力フレーム数が一致しません (expected {expected}, got {})",
                samples.len()
            ));
        }
    }
    Ok(ClipAudio {
        sample_rate: rate,
        channels,
        samples,
    })
}

/// プラグイントラックのレンダリング失敗（Issue #72）。
/// トラック単位で収集され、呼び出し側がエラー状態として表示する。
#[derive(Debug, Clone, PartialEq)]
pub struct PluginTrackError {
    pub track_id: u64,
    pub track_name: String,
    pub message: String,
}

/// ヘルパーバイナリの場所。実行ファイルと同じディレクトリの
/// `plugin-host` を優先し、無ければ PATH 上の名前で引く。
pub fn default_helper_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("plugin-host");
            if sibling.exists() {
                return sibling;
            }
        }
    }
    PathBuf::from("plugin-host")
}

/// プロジェクト内の全プラグイントラックを事前レンダリングする。
/// 戻り値は（音声マップへの追加分、トラック単位のエラー一覧）。
/// ノートの無いプラグイントラックは対象外（鳴らすものが無い）。
pub fn prepare_plugin_audio(
    project: &Project,
    helper: &Path,
    sample_rate: u32,
) -> (HashMap<String, ClipAudio>, Vec<PluginTrackError>) {
    let mut audio = HashMap::new();
    let mut errors = Vec::new();
    // レンダリング範囲はプロジェクト全体（先頭から最終クリップ終端）。
    let project_end_ms = project
        .tracks
        .iter()
        .flat_map(|t| t.clips.iter().map(|c| c.end_ms()))
        .max()
        .unwrap_or(0);
    for track in &project.tracks {
        if track.kind != TrackKind::Audio {
            continue;
        }
        let Some(Instrument::Plugin {
            format,
            plugin_path,
            preset_path,
        }) = &track.instrument
        else {
            continue;
        };
        if *format != PluginFormat::Clap {
            continue;
        }
        // トラック内の全ノートを絶対時刻で集める。
        let mut notes: Vec<NoteJson> = Vec::new();
        for clip in &track.clips {
            for note in &clip.notes {
                notes.push(NoteJson {
                    start_ms: (clip.start_ms + note.start_ms) as f64,
                    duration_ms: note.duration_ms as f64,
                    pitch: note.pitch,
                    velocity: note.velocity,
                });
            }
        }
        if notes.is_empty() {
            continue;
        }
        let fail = |errors: &mut Vec<PluginTrackError>, message: String| {
            errors.push(PluginTrackError {
                track_id: track.id,
                track_name: track.name.clone(),
                message,
            });
        };
        // プリセットはファイルの内容（JSON）をステートとして注入する。
        let preset_json = match preset_path {
            Some(path) => match std::fs::read_to_string(path) {
                Ok(text) => Some(text),
                Err(e) => {
                    fail(
                        &mut errors,
                        format!("プリセットを読み込めません: {path}: {e}"),
                    );
                    continue;
                }
            },
            None => None,
        };
        match render_notes(
            helper,
            plugin_path,
            preset_json,
            &notes,
            project_end_ms,
            sample_rate,
        ) {
            Ok(rendered) => {
                audio.insert(plugin_track_key(track.id), rendered);
            }
            Err(message) => fail(&mut errors, message),
        }
    }
    (audio, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::{apply, Command, Instrument, PluginFormat, TrackKind};

    const RATE: u32 = 48_000;

    /// ビルド成果物（plugin-host、フィクスチャの .so）を探す。
    /// 標準レイアウトではプロファイルディレクトリ（target/debug 等）の
    /// 直下に並ぶが、環境によっては build/<pkg>/<hash>/out/ に
    /// 置かれるため、直下に無ければ配下を再帰探索する。
    fn target_debug_file(name: &str) -> PathBuf {
        let exe = std::env::current_exe().unwrap();
        let profile_dir = exe
            .ancestors()
            .find(|dir| {
                matches!(
                    dir.file_name().and_then(|n| n.to_str()),
                    Some("debug") | Some("release")
                )
            })
            .map(|dir| dir.to_path_buf());
        if let Some(dir) = &profile_dir {
            let direct = dir.join(name);
            if direct.exists() {
                return direct;
            }
            if let Some(found) = find_under(dir, name, 5) {
                return found;
            }
        }
        PathBuf::from(name)
    }

    fn find_under(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
        if depth == 0 {
            return None;
        }
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_file() && path.file_name().and_then(|n| n.to_str()) == Some(name) {
                return Some(path);
            }
            if path.is_dir() {
                if let Some(found) = find_under(&path, name, depth - 1) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn helper_path() -> PathBuf {
        target_debug_file("plugin-host")
    }

    fn fixture_plugin_path() -> String {
        target_debug_file("libclap_test_synth.so")
            .to_string_lossy()
            .into_owned()
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    fn goertzel(samples: &[f32], rate: u32, freq: f32) -> f32 {
        let k = 2.0 * std::f32::consts::PI * freq / rate as f32;
        let coeff = 2.0 * k.cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for &x in samples {
            let s = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2).sqrt() / samples.len() as f32 * 2.0
    }

    fn project_with_plugin_track(plugin_path: &str, preset_path: Option<String>) -> (Project, u64) {
        let mut p = Project::new("plugin-test");
        let track_id = apply(
            &mut p,
            &Command::AddTrack {
                name: "VITAL".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        let clip_id = apply(
            &mut p,
            &Command::AddClip {
                track_id,
                name: "melody".into(),
                start_ms: 0,
                duration_ms: 1_000,
                source: String::new(),
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::AddNote {
                clip_id,
                pitch: 69,
                start_ms: 100,
                duration_ms: 500,
                velocity: 127,
            },
        )
        .unwrap();
        apply(
            &mut p,
            &Command::SetTrackInstrument {
                track_id,
                instrument: Some(Instrument::Plugin {
                    format: PluginFormat::Clap,
                    plugin_path: plugin_path.into(),
                    preset_path,
                }),
            },
        )
        .unwrap();
        (p, track_id)
    }

    #[test]
    fn prepare_renders_fixture_track_into_audio_map() {
        // Issue #72 スパイク: 代替 CLAP プラグイン（テスト用シンセ）で
        // パイプライン全体（別プロセス→ステート注入なし→ノート
        // レンダリング）が通り、440Hz が鳴る。Vital 実バイナリでの
        // 検証は未実施（アカウント経由配布のため入手不可）。
        let (p, track_id) = project_with_plugin_track(&fixture_plugin_path(), None);
        let (audio, errors) = prepare_plugin_audio(&p, &helper_path(), RATE);
        assert!(errors.is_empty(), "errors: {errors:?}");
        let rendered = audio
            .get(&crate::audio::plugin_track_key(track_id))
            .expect("plugin audio must be prepared");
        assert_eq!(rendered.sample_rate, RATE);
        let l: Vec<f32> = rendered.samples.iter().step_by(2).copied().collect();
        let during = &l[(0.2 * RATE as f32) as usize..(0.5 * RATE as f32) as usize];
        assert!(
            goertzel(during, RATE, 440.0) > 0.1,
            "A4 must sound through the plugin"
        );
        assert!((peak(during) - 0.5).abs() < 0.05, "default gain 0.5");
        let after = &l[(0.7 * RATE as f32) as usize..(0.9 * RATE as f32) as usize];
        assert!(peak(after) < 1e-4, "silent after note off");
    }

    #[test]
    fn prepare_applies_preset_json_to_plugin_state() {
        // プリセット（.vital 相当の JSON）をステートとして注入すると
        // 出力が変わる（フィクスチャでは gain と波形が変わる）。
        let dir = std::env::temp_dir().join(format!("rs-plugin-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let preset = dir.join("preset.vital");
        std::fs::write(&preset, "{\"gain\": 0.25, \"square\": true}").unwrap();
        let (p, track_id) = project_with_plugin_track(
            &fixture_plugin_path(),
            Some(preset.to_string_lossy().into_owned()),
        );
        let (audio, errors) = prepare_plugin_audio(&p, &helper_path(), RATE);
        assert!(errors.is_empty(), "errors: {errors:?}");
        let rendered = audio
            .get(&crate::audio::plugin_track_key(track_id))
            .unwrap();
        let l: Vec<f32> = rendered.samples.iter().step_by(2).copied().collect();
        let during = &l[(0.2 * RATE as f32) as usize..(0.5 * RATE as f32) as usize];
        assert!(
            (peak(during) - 0.25).abs() < 0.03,
            "preset gain 0.25 must apply, peak was {}",
            peak(during)
        );
    }

    #[test]
    fn prepare_reports_missing_plugin_as_track_error() {
        // プラグインが無い環境: 明確なエラーになり、音声は入らない。
        let (p, track_id) = project_with_plugin_track("/nonexistent/Vital.clap", None);
        let (audio, errors) = prepare_plugin_audio(&p, &helper_path(), RATE);
        assert!(audio.is_empty());
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].track_id, track_id);
        assert!(!errors[0].message.is_empty());
    }

    #[test]
    fn prepare_reports_missing_preset_as_track_error() {
        let (p, _track) = project_with_plugin_track(
            &fixture_plugin_path(),
            Some("/nonexistent/preset.vital".into()),
        );
        let (audio, errors) = prepare_plugin_audio(&p, &helper_path(), RATE);
        assert!(audio.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0].message.contains("preset.vital"),
            "error must name the preset: {}",
            errors[0].message
        );
    }

    #[test]
    fn prepare_reports_missing_helper_as_track_error() {
        let (p, _track) = project_with_plugin_track(&fixture_plugin_path(), None);
        let (audio, errors) = prepare_plugin_audio(&p, Path::new("/nonexistent/plugin-host"), RATE);
        assert!(audio.is_empty());
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn prepare_ignores_sampler_and_unset_instruments() {
        // プラグインでないトラックは対象外で、エラーにもならない。
        let mut p = Project::new("plain");
        let track_id = apply(
            &mut p,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        let clip_id = apply(
            &mut p,
            &Command::AddClip {
                track_id,
                name: "melody".into(),
                start_ms: 0,
                duration_ms: 1_000,
                source: String::new(),
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::AddNote {
                clip_id,
                pitch: 69,
                start_ms: 0,
                duration_ms: 500,
                velocity: 127,
            },
        )
        .unwrap();
        let (audio, errors) = prepare_plugin_audio(&p, &helper_path(), RATE);
        assert!(audio.is_empty());
        assert!(errors.is_empty());
    }
}
