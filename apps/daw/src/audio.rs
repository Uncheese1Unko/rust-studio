//! daw の音声再生エンジン（Issue #13）。
//!
//! WAV のデコード、プロジェクトのミックスレンダリング、再生位置の
//! 状態機械を持つ。実デバイスへの出力はこの層の外（本 PBI では
//! レンダリング結果と状態遷移をテストで固定し、実音は実機未検証）。

use std::collections::HashMap;

use studio_core::Project;

/// デコード済みの音声。サンプルはインターリーブ f32。
#[derive(Debug, Clone, PartialEq)]
pub struct ClipAudio {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl ClipAudio {
    pub fn duration_ms(&self) -> i64 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0;
        }
        let frames = self.samples.len() as i64 / self.channels as i64;
        frames * 1000 / self.sample_rate as i64
    }
}

/// WAV ファイルをデコードする。
pub fn decode_audio(path: &std::path::Path) -> Result<ClipAudio, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?,
        hound::SampleFormat::Int => {
            // hound の i32 読みはファイルのビット深度そのままの値域で
            // 返るため、正規化は i32 の最大値ではなくビット深度で行う
            // （Issue #13 のデモ実行で検出）。
            let scale = (1u64 << (spec.bits_per_sample.saturating_sub(1)).min(31)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        }
    };
    Ok(ClipAudio {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        samples,
    })
}

/// クリップ音声のピッチを変更する（Issue #18、方式は ADR-0009:
/// Signalsmith Stretch、フォルマント係数 1.0 で音の性質を保つ）。
/// 長さ（フレーム数）は変わらない。0 半音はそのまま返す。
pub fn pitch_shift(audio: &ClipAudio, semitones: f32) -> ClipAudio {
    if semitones == 0.0 || audio.samples.is_empty() || audio.channels == 0 {
        return audio.clone();
    }
    let mut stretch =
        signalsmith_stretch::Stretch::preset_default(audio.channels as u32, audio.sample_rate);
    stretch.set_transpose_factor_semitones(semitones, None);
    stretch.set_formant_factor(1.0, true);
    let mut out = vec![0.0f32; audio.samples.len()];
    stretch.exact(&audio.samples, &mut out);
    ClipAudio {
        sample_rate: audio.sample_rate,
        channels: audio.channels,
        samples: out,
    }
}

/// フェード包絡（Issue #34）: クリップ内位置に対する直線フェードの
/// 倍率。フェードインは区間内で 0→1、フェードアウトは 1→0。
/// 両方が重なる区間では小さい方を取る。
pub fn fade_envelope(clip: &studio_core::Clip, within_ms: f64) -> f32 {
    let mut env = 1.0f64;
    if clip.fade_in_ms > 0 {
        env = env.min((within_ms / clip.fade_in_ms as f64).clamp(0.0, 1.0));
    }
    if clip.fade_out_ms > 0 {
        let rest = clip.duration_ms as f64 - within_ms;
        env = env.min((rest / clip.fade_out_ms as f64).clamp(0.0, 1.0));
    }
    env as f32
}

/// プロジェクトのミックスを指定範囲でレンダリングする（ステレオ出力）。
/// 配置位置・ゲインはプロジェクトの状態どおりに反映される。
/// トラックが再生ミックスで鳴るかどうか（Issue #39）。
/// ミュートは常に出ない。ソロが1つでもあるプロジェクトでは
/// ソロでないトラックは出ない（ミュート優先）。
pub fn is_track_audible(track: &studio_core::Track, any_solo: bool) -> bool {
    if track.muted {
        return false;
    }
    !any_solo || track.solo
}

/// プラグイン楽器トラックの事前レンダリング済み音声を音声マップに
/// 置くときのキー（Issue #72、ADR-0017）。レンダリングは
/// `plugin::prepare_plugin_audio` がプロジェクト先頭からの
/// 絶対時刻で用意し、`render_mix` はこのキーで引いて使う。
pub fn plugin_track_key(track_id: u64) -> String {
    format!("plugin://track/{track_id}")
}

pub fn render_mix(
    project: &Project,
    audio: &HashMap<String, ClipAudio>,
    start_ms: i64,
    duration_ms: i64,
    out_rate: u32,
) -> Vec<f32> {
    let frames = (duration_ms.max(0) as u64 * out_rate as u64 / 1000) as usize;
    let mut out = vec![0.0f32; frames * 2];
    // ミュート／ソロの判定（Issue #39）: ソロの有無はプロジェクト全体で決まる。
    let any_solo = project.tracks.iter().any(|t| t.solo);
    // ピッチ変更済みクリップ音声のキャッシュ（このレンダリング内のみ）。
    let mut pitch_cache: HashMap<(String, u32), ClipAudio> = HashMap::new();
    // 実効楽器（Issue #73、ADR-0018）: 音色の割り当てを解決した
    // 楽器をトラック位置で引けるよう先に確定させる（フレーム毎に
    // 解決し直さない）。
    let effective: Vec<Option<studio_core::Instrument>> = project
        .tracks
        .iter()
        .map(|t| project.effective_instrument(t))
        .collect();
    for (i, frame) in out.chunks_mut(2).enumerate() {
        // 時刻は f64 で持つ。整数ミリ秒に丸めると1ms ごとに波形が階段状に
        // なり、音程が壊れる（Issue #13 のテストで検出）。
        let t_ms = start_ms as f64 + i as f64 * 1000.0 / out_rate as f64;
        for (track_index, track) in project.tracks.iter().enumerate() {
            if track.kind != studio_core::TrackKind::Audio {
                continue;
            }
            if !is_track_audible(track, any_solo) {
                continue;
            }
            // トラック音量とパン（Issue #15）。パンはバランス方式:
            // 中央では左右とも等倍、端では反対側が 0 になる。
            let channel_factor = [
                track.volume * (1.0 - track.pan.max(0.0)),
                track.volume * (1.0 + track.pan.min(0.0)),
            ];
            for clip in &track.clips {
                if t_ms < clip.start_ms as f64 || t_ms >= (clip.start_ms + clip.duration_ms) as f64
                {
                    continue;
                }
                if !clip.notes.is_empty() {
                    // ノートクリップ（Issue #68、ADR-0016）: 波形では
                    // なく、トラックの楽器でノートごとに発音する。
                    // 楽器が無いトラックのノートは鳴らない。
                    if let Some(studio_core::Instrument::Plugin { .. }) = &effective[track_index] {
                        // プラグイン楽器（Issue #72、ADR-0017）:
                        // 事前レンダリング済みのトラック音声
                        // （プロジェクト先頭からの絶対時刻）を
                        // クリップの区間だけ、クリップのゲインと
                        // フェードを掛けて鳴らす。事前レンダリングが
                        // 無い場合は無音（失敗は prepare_plugin_audio
                        // がエラーとして明示する）。
                        if let Some(rendered) = audio.get(&plugin_track_key(track.id)) {
                            if rendered.channels > 0 && rendered.sample_rate > 0 {
                                let pos = t_ms * rendered.sample_rate as f64 / 1000.0;
                                let i0 = pos.floor() as usize;
                                let frac = (pos - i0 as f64) as f32;
                                let ch = rendered.channels as usize;
                                let gain =
                                    clip.gain * fade_envelope(clip, t_ms - clip.start_ms as f64);
                                for (out_ch, slot) in frame.iter_mut().enumerate() {
                                    let src_ch = out_ch.min(ch - 1);
                                    let a = rendered
                                        .samples
                                        .get(i0 * ch + src_ch)
                                        .copied()
                                        .unwrap_or(0.0);
                                    let b = rendered
                                        .samples
                                        .get((i0 + 1) * ch + src_ch)
                                        .copied()
                                        .unwrap_or(0.0);
                                    *slot += (a + (b - a) * frac) * gain * channel_factor[out_ch];
                                }
                            }
                        }
                        continue;
                    }
                    let Some(studio_core::Instrument::Sampler { source, root_note }) =
                        &effective[track_index]
                    else {
                        continue;
                    };
                    let Some(base_audio) = audio.get(source) else {
                        continue;
                    };
                    if base_audio.channels == 0 || base_audio.sample_rate == 0 {
                        continue;
                    }
                    for note in &clip.notes {
                        let note_start = clip.start_ms as f64 + note.start_ms as f64;
                        let note_end = note_start + note.duration_ms as f64;
                        if t_ms < note_start || t_ms >= note_end {
                            continue;
                        }
                        // 音高シフト量は note.pitch - root_note（半音）。
                        // ピッチ変更済み音声はクリップのピッチと
                        // キャッシュを共有する（キーは素材＋半音数）。
                        let semitones = (note.pitch as i32 - *root_note as i32) as f32;
                        let note_audio: &ClipAudio = if semitones != 0.0 {
                            pitch_cache
                                .entry((source.clone(), semitones.to_bits()))
                                .or_insert_with(|| pitch_shift(base_audio, semitones))
                        } else {
                            base_audio
                        };
                        // 素材はノートの先頭から鳴らす（クリップの
                        // offset_ms は波形クリップ専用で、ノートには
                        // 適用しない）。素材が尽きたら無音に倒れる。
                        let pos = (t_ms - note_start) * note_audio.sample_rate as f64 / 1000.0;
                        let i0 = pos.floor() as usize;
                        let frac = (pos - i0 as f64) as f32;
                        let ch = note_audio.channels as usize;
                        // ベロシティは 0–127 を 0.0–1.0 のゲインに
                        // 線形変換する。クリップのゲインとフェードも
                        // 波形クリップと同じに効く。
                        let note_gain = clip.gain
                            * (note.velocity as f32 / 127.0)
                            * fade_envelope(clip, t_ms - clip.start_ms as f64);
                        for (out_ch, slot) in frame.iter_mut().enumerate() {
                            let src_ch = out_ch.min(ch - 1);
                            let a = note_audio
                                .samples
                                .get(i0 * ch + src_ch)
                                .copied()
                                .unwrap_or(0.0);
                            let b = note_audio
                                .samples
                                .get((i0 + 1) * ch + src_ch)
                                .copied()
                                .unwrap_or(0.0);
                            *slot += (a + (b - a) * frac) * note_gain * channel_factor[out_ch];
                        }
                    }
                    continue;
                }
                let Some(base_audio) = audio.get(&clip.source) else {
                    continue;
                };
                if base_audio.channels == 0 || base_audio.sample_rate == 0 {
                    continue;
                }
                // ピッチ変更（Issue #18）: 指定があれば変更済み音声を使う。
                let clip_audio = if clip.pitch_semitones != 0.0 {
                    pitch_cache
                        .entry((clip.source.clone(), clip.pitch_semitones.to_bits()))
                        .or_insert_with(|| pitch_shift(base_audio, clip.pitch_semitones))
                } else {
                    base_audio
                };
                // クリップ内位置＋素材内オフセット（Issue #32）→
                // ソースのフレーム位置（線形補間）
                let pos = (t_ms - clip.start_ms as f64 + clip.offset_ms as f64)
                    * clip_audio.sample_rate as f64
                    / 1000.0;
                let i0 = pos.floor() as usize;
                let frac = (pos - i0 as f64) as f32;
                let ch = clip_audio.channels as usize;
                for (out_ch, slot) in frame.iter_mut().enumerate() {
                    let src_ch = out_ch.min(ch - 1);
                    let a = clip_audio
                        .samples
                        .get(i0 * ch + src_ch)
                        .copied()
                        .unwrap_or(0.0);
                    let b = clip_audio
                        .samples
                        .get((i0 + 1) * ch + src_ch)
                        .copied()
                        .unwrap_or(0.0);
                    *slot += (a + (b - a) * frac)
                        * clip.gain
                        * fade_envelope(clip, t_ms - clip.start_ms as f64)
                        * channel_factor[out_ch];
                }
            }
        }
        for slot in frame.iter_mut() {
            *slot = slot.clamp(-1.0, 1.0);
        }
    }
    out
}

/// サンプル列のピーク（絶対値の最大、Issue #27）。レベルメーターの
/// 入力で、読み取り専用（音声処理そのものには手を入れない）。
/// メトロノームのクリックの振幅（固定、Issue #36）。
pub const CLICK_AMPLITUDE: f32 = 0.5;
/// クリックの長さ（ミリ秒）。減衰しきるまでの長さ。
pub const CLICK_DURATION_MS: f64 = 30.0;
/// 強拍（小節頭）のクリック周波数（Hz）。
pub const CLICK_ACCENT_HZ: f64 = 2_000.0;
/// 弱拍のクリック周波数（Hz）。
pub const CLICK_BEAT_HZ: f64 = 1_000.0;

/// クリック波形（Issue #36）: 短いサイン波に指数減衰を掛けたもの。
/// `t_ms` はクリック開始からの経過ミリ秒。
pub fn click_waveform(freq_hz: f64, t_ms: f64) -> f32 {
    if !(0.0..CLICK_DURATION_MS).contains(&t_ms) {
        return 0.0;
    }
    let phase = 2.0 * std::f64::consts::PI * freq_hz * t_ms / 1000.0;
    (phase.sin() as f32) * (-t_ms / 8.0).exp() as f32 * CLICK_AMPLITUDE
}

/// ミックスへメトロノームのクリックを合成する（Issue #36）。
/// 拍はプロジェクト先頭（0ms）から数え、小節頭（4拍ごと）が強拍。
/// `mix` はステレオインターリーブ、`start_ms` はミックス先頭の
/// プロジェクト時刻。合成後に ±1 へクランプする。
pub fn add_metronome_clicks(mix: &mut [f32], bpm: f32, start_ms: i64, out_rate: u32) {
    let beat_ms = 60_000.0 / bpm as f64;
    let frames = mix.len() / 2;
    let duration_ms = frames as f64 * 1000.0 / out_rate as f64;
    let first_beat = (start_ms as f64 / beat_ms).ceil() as i64;
    let mut k = first_beat.max(0);
    loop {
        let beat_time = k as f64 * beat_ms;
        if beat_time >= start_ms as f64 + duration_ms {
            break;
        }
        let freq = if k % 4 == 0 {
            CLICK_ACCENT_HZ
        } else {
            CLICK_BEAT_HZ
        };
        let offset_frames =
            ((beat_time - start_ms as f64) * out_rate as f64 / 1000.0).round() as usize;
        let click_frames = (CLICK_DURATION_MS * out_rate as f64 / 1000.0).ceil() as usize;
        for i in 0..click_frames {
            let frame = offset_frames + i;
            if frame >= frames {
                break;
            }
            let t_ms = i as f64 * 1000.0 / out_rate as f64;
            let v = click_waveform(freq, t_ms);
            mix[frame * 2] += v;
            mix[frame * 2 + 1] += v;
        }
        k += 1;
    }
    for s in mix.iter_mut() {
        *s = s.clamp(-1.0, 1.0);
    }
}

/// ミックスを WAV に書き出す（Issue #41）。プロジェクト全長
/// （最後のクリップの終端まで）を 44.1kHz / 16bit PCM / ステレオで
/// 書き出す。中身は render_mix の出力そのもの（16bit 量子化のみ）。
pub fn export_wav(
    project: &Project,
    audio: &HashMap<String, ClipAudio>,
    out_path: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    const RATE: u32 = 44_100;
    let end_ms = crate::timeline::project_end_ms(project);
    let mix = render_mix(project, audio, 0, end_ms, RATE);
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(out_path, spec).map_err(|e| e.to_string())?;
    for sample in &mix {
        let v = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        writer.write_sample(v).map_err(|e| e.to_string())?;
    }
    writer.finalize().map_err(|e| e.to_string())?;
    Ok(out_path.to_path_buf())
}

/// 書き出し先の初期値（Issue #41）: プロジェクトファイルと同じ
/// ディレクトリの `<プロジェクト名>.wav`。プロジェクトファイルが
/// なければカレントディレクトリの同名ファイル。
pub fn default_export_path(
    project_file: Option<&std::path::Path>,
    project_name: &str,
) -> std::path::PathBuf {
    let file_name = format!("{project_name}.wav");
    match project_file.and_then(|p| p.parent()) {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(file_name),
        _ => std::path::PathBuf::from(file_name),
    }
}

/// render_mix にメトロノーム（Issue #36）を重ねた再生レンダリング。
/// ON/OFF はアプリ側の再生状態で、プロジェクトには保存しない。
pub fn render_mix_with_metronome(
    project: &Project,
    audio: &HashMap<String, ClipAudio>,
    start_ms: i64,
    duration_ms: i64,
    out_rate: u32,
    metronome_on: bool,
) -> Vec<f32> {
    let mut mix = render_mix(project, audio, start_ms, duration_ms, out_rate);
    if metronome_on {
        add_metronome_clicks(&mut mix, project.bpm, start_ms, out_rate);
    }
    mix
}

pub fn peak_of(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |acc, s| acc.max(s.abs()))
}

/// ピーク付近とみなすレベル（dB）。これ以上はメーターを強調色にする。
pub const PEAK_LEVEL_DB: f32 = -3.0;

/// レベルメーターの表示下限（dB）。これ以下はメーターが空になる。
pub const METER_FLOOR_DB: f32 = -60.0;

/// ピーク値の dB 換算（1.0 = 0dB）。0 は -inf。
pub fn peak_db(peak: f32) -> f32 {
    if peak <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * peak.log10()
    }
}

/// その dB がピーク付近（`PEAK_LEVEL_DB` 以上）かどうか。
pub fn is_peak_level(db: f32) -> bool {
    db >= PEAK_LEVEL_DB
}

/// 再生位置の状態機械。実時間の進行は呼び出し側が advance で与える。
#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub playing: bool,
    pub position_ms: i64,
    pub project_end_ms: i64,
}

impl Player {
    pub fn new(project_end_ms: i64) -> Self {
        Player {
            playing: false,
            position_ms: 0,
            project_end_ms,
        }
    }
    pub fn play(&mut self) {
        self.playing = true;
    }
    pub fn pause(&mut self) {
        self.playing = false;
    }
    pub fn seek(&mut self, ms: i64) {
        self.position_ms = ms.clamp(0, self.project_end_ms);
    }
    pub fn advance(&mut self, dt_ms: i64) {
        if !self.playing {
            return;
        }
        self.position_ms += dt_ms.max(0);
        if self.position_ms >= self.project_end_ms {
            self.position_ms = self.project_end_ms;
            self.playing = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::{apply, Command, TrackKind};

    pub(super) const RATE: u32 = 44100;

    pub(super) fn sine(freq: f32, ms: i64, amp: f32) -> ClipAudio {
        let frames = (RATE as i64 * ms / 1000) as usize;
        let samples = (0..frames)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / RATE as f32).sin())
            .collect();
        ClipAudio {
            sample_rate: RATE,
            channels: 1,
            samples,
        }
    }

    pub(super) fn goertzel(samples: &[f32], rate: u32, freq: f32) -> f32 {
        let n = samples.len() as f32;
        let k = 2.0 * std::f32::consts::PI * freq / rate as f32;
        let coeff = 2.0 * k.cos();
        let (mut s1, mut s2) = (0.0f32, 0.0f32);
        for &x in samples {
            let s = x + coeff * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        (s1 * s1 + s2 * s2 - coeff * s1 * s2).sqrt() / n
    }

    pub(super) fn window(
        samples: &[f32],
        channels: usize,
        from_ms: i64,
        to_ms: i64,
        rate: u32,
    ) -> Vec<f32> {
        let a = (from_ms * rate as i64 / 1000) as usize * channels;
        let b = (to_ms * rate as i64 / 1000) as usize * channels;
        // 左チャンネルだけ取り出す
        samples[a..b].iter().step_by(channels).cloned().collect()
    }

    pub(super) fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    pub(super) fn project_with(
        clips: &[(&str, i64, i64, f32)],
    ) -> (Project, HashMap<String, ClipAudio>) {
        let mut p = Project::new("engine test");
        let track = apply(
            &mut p,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        let mut audio = HashMap::new();
        audio.insert("a.wav".to_string(), sine(440.0, 3000, 0.4));
        audio.insert("b.wav".to_string(), sine(880.0, 3000, 0.4));
        for (source, start, dur, gain) in clips {
            let id = apply(
                &mut p,
                &Command::AddClip {
                    track_id: track,
                    name: source.to_string(),
                    start_ms: *start,
                    duration_ms: *dur,
                    source: source.to_string(),
                },
            )
            .unwrap()
            .unwrap();
            if *gain != 1.0 {
                apply(
                    &mut p,
                    &Command::SetClipGain {
                        clip_id: id,
                        gain: *gain,
                    },
                )
                .unwrap();
            }
        }
        (p, audio)
    }

    #[test]
    fn decode_roundtrip_reads_wav() {
        let path = std::env::temp_dir().join(format!("daw-decode-{}.wav", std::process::id()));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for s in sine(440.0, 100, 0.5).samples {
            writer.write_sample((s * i16::MAX as f32) as i16).unwrap();
        }
        writer.finalize().unwrap();
        let decoded = decode_audio(&path).unwrap();
        assert_eq!(decoded.sample_rate, RATE);
        assert_eq!(decoded.channels, 1);
        assert!((decoded.duration_ms() - 100).abs() <= 1);
        // 440Hz・振幅0.5 の10サンプル目 = 0.5*sin(2π*440*10/44100) ≈ 0.293
        assert!(
            (decoded.samples[10] - 0.293).abs() < 0.01,
            "sample[10] was {}",
            decoded.samples[10]
        );
        // 書き込んだサインの振幅（0.5）が保たれていること。
        // ビット深度に対する正規化の誤りは、ここで捕まえる。
        let max = decoded.samples.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!((max - 0.5).abs() < 0.02, "decoded amplitude was {max}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn render_places_clips_at_their_positions() {
        let (p, audio) = project_with(&[("a.wav", 0, 1000, 1.0), ("b.wav", 500, 1000, 1.0)]);
        let mix = render_mix(&p, &audio, 0, 2000, RATE);
        // 100-400ms: 440 のみ
        let w = window(&mix, 2, 100, 400, RATE);
        assert!(goertzel(&w, RATE, 440.0) > 0.1, "440 should sound");
        assert!(goertzel(&w, RATE, 880.0) < 0.03, "880 should be silent");
        // 600-900ms: 両方
        let w = window(&mix, 2, 600, 900, RATE);
        assert!(goertzel(&w, RATE, 440.0) > 0.1);
        assert!(goertzel(&w, RATE, 880.0) > 0.1);
        // 1100-1400ms: 880 のみ
        let w = window(&mix, 2, 1100, 1400, RATE);
        assert!(goertzel(&w, RATE, 440.0) < 0.03);
        assert!(goertzel(&w, RATE, 880.0) > 0.1);
        // 1600ms 以降: 無音
        let w = window(&mix, 2, 1600, 1900, RATE);
        assert!(rms(&w) < 1e-4);
    }

    #[test]
    fn render_applies_clip_gain() {
        let (p1, audio) = project_with(&[("a.wav", 0, 1000, 1.0)]);
        let (p2, _) = project_with(&[("a.wav", 0, 1000, 0.5)]);
        let m1 = render_mix(&p1, &audio, 0, 1000, RATE);
        let m2 = render_mix(&p2, &audio, 0, 1000, RATE);
        let ratio = rms(&window(&m2, 2, 100, 900, RATE)) / rms(&window(&m1, 2, 100, 900, RATE));
        assert!((ratio - 0.5).abs() < 0.05, "gain ratio was {ratio}");
    }

    #[test]
    fn overlapping_identical_clips_sum() {
        let (p1, audio) = project_with(&[("a.wav", 0, 1000, 1.0)]);
        let (p2, _) = project_with(&[("a.wav", 0, 1000, 1.0), ("a.wav", 0, 1000, 1.0)]);
        let m1 = render_mix(&p1, &audio, 0, 1000, RATE);
        let m2 = render_mix(&p2, &audio, 0, 1000, RATE);
        let ratio = rms(&window(&m2, 2, 100, 900, RATE)) / rms(&window(&m1, 2, 100, 900, RATE));
        assert!((ratio - 2.0).abs() < 0.1, "sum ratio was {ratio}");
    }

    #[test]
    fn peak_and_db_of_known_samples() {
        // Issue #27: レベルメーターの基礎。既知のサンプル列に対する
        // ピーク値と dB 変換、ピーク付近（-3dB 以上）の判定を固定する。
        assert_eq!(peak_of(&[]), 0.0);
        assert_eq!(peak_of(&[0.1, -0.5, 0.25]), 0.5);
        assert_eq!(peak_of(&[-0.25, 0.125]), 0.25);
        assert!((peak_db(1.0) - 0.0).abs() < 1e-4);
        assert!((peak_db(0.5) - -6.0206).abs() < 0.001);
        assert_eq!(peak_db(0.0), f32::NEG_INFINITY);
        // 0.71 は約 -2.97dB（ピーク付近）、0.5 は約 -6.02dB（通常）
        assert!(is_peak_level(peak_db(0.71)));
        assert!(!is_peak_level(peak_db(0.5)));
    }

    #[test]
    fn player_state_transitions() {
        let mut player = Player::new(3000);
        assert!(!player.playing && player.position_ms == 0);
        player.play();
        player.advance(500);
        assert!(player.playing && player.position_ms == 500);
        player.pause();
        player.advance(500);
        assert_eq!(player.position_ms, 500, "paused player must not move");
        player.seek(2900);
        player.play();
        player.advance(500);
        assert_eq!(player.position_ms, 3000, "position clamps at project end");
        assert!(!player.playing, "playback stops at project end");
        player.seek(-100);
        assert_eq!(player.position_ms, 0, "seek clamps at 0");
    }
}

#[cfg(test)]
mod mixer_tests {
    use super::*;
    use studio_core::{apply, Command, TrackKind};

    fn project_with_track() -> (Project, HashMap<String, ClipAudio>) {
        let mut p = Project::new("mixer test");
        let track = apply(
            &mut p,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::AddClip {
                track_id: track,
                name: "tone".into(),
                start_ms: 0,
                duration_ms: 1000,
                source: "a.wav".into(),
            },
        )
        .unwrap();
        let mut audio = HashMap::new();
        audio.insert(
            "a.wav".to_string(),
            ClipAudio {
                sample_rate: 44100,
                channels: 1,
                samples: (0..44100)
                    .map(|i| 0.4 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 44100.0).sin())
                    .collect(),
            },
        );
        (p, audio)
    }

    fn channel_rms(mix: &[f32], channel: usize) -> f32 {
        let samples: Vec<f32> = mix.iter().skip(channel).step_by(2).cloned().collect();
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    #[test]
    fn track_volume_scales_the_mix() {
        let (mut p, audio) = project_with_track();
        let base = render_mix(&p, &audio, 0, 1000, 44100);
        apply(
            &mut p,
            &Command::SetTrackVolume {
                track_id: 1,
                volume: 0.5,
            },
        )
        .unwrap();
        let halved = render_mix(&p, &audio, 0, 1000, 44100);
        let ratio = channel_rms(&halved, 0) / channel_rms(&base, 0);
        assert!((ratio - 0.5).abs() < 0.03, "volume ratio was {ratio}");
    }

    #[test]
    fn track_pan_moves_the_balance() {
        let (mut p, audio) = project_with_track();
        // 左いっぱい: 右は無音、左はそのまま
        apply(
            &mut p,
            &Command::SetTrackPan {
                track_id: 1,
                pan: -1.0,
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 1000, 44100);
        assert!(channel_rms(&mix, 1) < 1e-5, "right must be silent");
        assert!(channel_rms(&mix, 0) > 0.2, "left must sound");
        // 右いっぱい: 逆
        apply(
            &mut p,
            &Command::SetTrackPan {
                track_id: 1,
                pan: 1.0,
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 1000, 44100);
        assert!(channel_rms(&mix, 0) < 1e-5, "left must be silent");
        assert!(channel_rms(&mix, 1) > 0.2, "right must sound");
        // 中央: 左右が等しい
        apply(
            &mut p,
            &Command::SetTrackPan {
                track_id: 1,
                pan: 0.0,
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 1000, 44100);
        let (l, r) = (channel_rms(&mix, 0), channel_rms(&mix, 1));
        assert!(
            (l - r).abs() < 1e-4,
            "center pan must be balanced: {l} vs {r}"
        );
    }
}

#[cfg(test)]
mod pitch_tests {
    use super::tests::*;
    use super::*;
    use studio_core::{apply, Command, TrackKind};

    /// 50ms 窓の Goertzel 平均。長い窓の位相崩れを避ける測定用。
    fn segmented_goertzel(samples: &[f32], rate: u32, freq: f32) -> f32 {
        let seg = rate as usize / 20;
        let mut total = 0.0f32;
        let mut count = 0usize;
        for chunk in samples.chunks(seg) {
            if chunk.len() == seg {
                total += goertzel(chunk, rate, freq);
                count += 1;
            }
        }
        total / count.max(1) as f32
    }

    #[test]
    fn pitch_shift_octave_up_doubles_frequency_and_keeps_duration() {
        // Issue #18: Signalsmith Stretch（ADR-0009）によるピッチ変更。
        let src = sine(220.0, 1000, 0.4);
        let out = pitch_shift(&src, 12.0);
        assert_eq!(out.sample_rate, RATE);
        assert_eq!(out.channels, 1);
        let ratio = out.samples.len() as f32 / src.samples.len() as f32;
        assert!(
            (ratio - 1.0).abs() < 0.02,
            "duration must be preserved, ratio was {ratio}"
        );
        // 中央の安定区間で測る（端の過渡を避ける）。長い窓では
        // 周波数の微小な誤差で位相が崩れて過小評価になるため、
        // 50ms の短い窓の平均で測る。
        let mid = &out.samples[RATE as usize / 4..RATE as usize * 3 / 4];
        assert!(
            segmented_goertzel(mid, RATE, 440.0) > 0.05,
            "440 should dominate after +12"
        );
        assert!(
            segmented_goertzel(mid, RATE, 220.0) < 0.01,
            "220 should be gone after +12"
        );
        // レベルが壊滅的に落ちていないこと。なお ADR-0009 の設定
        // （フォルマント係数 1.0、compensate あり）は狭帯域の信号で
        // 約 -8.5dB のレベル低下を伴うことが実測で分かっている
        // （試験レポートに記録）。広帯域の声では本テストの
        // フォルマント保持が効く。
        let in_rms = rms(&src.samples);
        let out_rms = rms(mid);
        assert!(
            out_rms > in_rms * 0.25,
            "output level collapsed: in {in_rms}, out {out_rms}"
        );
    }

    #[test]
    fn pitch_shift_octave_down_halves_frequency() {
        let src = sine(440.0, 1000, 0.4);
        let out = pitch_shift(&src, -12.0);
        let mid = &out.samples[RATE as usize / 4..RATE as usize * 3 / 4];
        assert!(goertzel(mid, RATE, 220.0) > 0.1);
        assert!(goertzel(mid, RATE, 440.0) < 0.03);
    }

    /// 声らしい信号: 基本周波数の倍音列に、フォルマントの包絡を載せる。
    fn voice_like(f0: f32) -> ClipAudio {
        let formants = [
            (730.0f32, 150.0f32, 1.0f32),
            (1090.0, 200.0, 0.6),
            (2440.0, 300.0, 0.3),
        ];
        let frames = RATE as usize; // 1 秒
        let mut samples = vec![0.0f32; frames];
        for k in 1..40 {
            let freq = f0 * k as f32;
            if freq > 4000.0 {
                break;
            }
            let mut amp = 0.15f32;
            for (fc, sigma, gain) in formants {
                let d = (freq - fc) / sigma;
                amp += gain * (-d * d).exp();
            }
            let amp = amp / k as f32;
            for (i, s) in samples.iter_mut().enumerate() {
                *s += amp * (2.0 * std::f32::consts::PI * freq * i as f32 / RATE as f32).sin();
            }
        }
        let max = samples.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        for s in samples.iter_mut() {
            *s *= 0.5 / max;
        }
        ClipAudio {
            sample_rate: RATE,
            channels: 1,
            samples,
        }
    }

    /// 倍音の大きさから包絡のピーク（フォルマント）位置を推定する。
    fn envelope_peak(samples: &[f32], f0: f32, lo: f32, hi: f32) -> f32 {
        let mut best = (0.0f32, 0.0f32);
        let mut k = 1;
        loop {
            let freq = f0 * k as f32;
            if freq > hi {
                break;
            }
            if freq >= lo {
                let mag = goertzel(samples, RATE, freq);
                if mag > best.1 {
                    best = (freq, mag);
                }
            }
            k += 1;
        }
        best.0
    }

    #[test]
    fn pitch_shift_preserves_formant_position() {
        // +12 半音で基本周波数は 120→240 になるが、フォルマント
        // （730Hz 付近の包絡ピーク）は元の位置に留まること。
        // 単純リサンプリングなら包絡ごと 2 倍へずれる。
        let src = voice_like(120.0);
        let before = envelope_peak(&src.samples, 120.0, 400.0, 1300.0);
        assert!(
            (before - 730.0).abs() < 150.0,
            "test signal formant should start near 730, got {before}"
        );
        let out = pitch_shift(&src, 12.0);
        let mid = &out.samples[RATE as usize / 4..RATE as usize * 3 / 4];
        // 基本周波数が 240 へ移ったこと
        assert!(goertzel(mid, RATE, 240.0) > goertzel(mid, RATE, 120.0));
        let after = envelope_peak(mid, 240.0, 400.0, 1300.0);
        assert!(
            (after - 730.0).abs() < 200.0,
            "formant must stay near 730 after +12, got {after}"
        );
    }

    #[test]
    fn muted_track_is_silent_and_solo_isolates() {
        // Issue #39: 2トラック（A=440Hz、B=880Hz）。ミュートした
        // トラックはミックスに出ず、ソロがあればソロ以外は出ない。
        // ミュートはソロより優先する。
        let mut p = Project::new("mute solo");
        let ta = apply(
            &mut p,
            &Command::AddTrack {
                name: "A".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        let tb = apply(
            &mut p,
            &Command::AddTrack {
                name: "B".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        for (tid, src) in [(ta, "a.wav"), (tb, "b.wav")] {
            apply(
                &mut p,
                &Command::AddClip {
                    track_id: tid,
                    name: src.into(),
                    start_ms: 0,
                    duration_ms: 2_000,
                    source: src.into(),
                },
            )
            .unwrap();
        }
        let mut audio = HashMap::new();
        audio.insert("a.wav".to_string(), sine(440.0, 3_000, 0.4));
        audio.insert("b.wav".to_string(), sine(880.0, 3_000, 0.4));
        let mag = |p: &Project, freq: f32| {
            let mix = render_mix(p, &audio, 0, 1_000, RATE);
            goertzel(&window(&mix, 2, 0, 1_000, RATE), RATE, freq)
        };
        assert!(
            mag(&p, 440.0) > 0.05 && mag(&p, 880.0) > 0.05,
            "both audible"
        );
        apply(
            &mut p,
            &Command::SetTrackMute {
                track_id: ta,
                muted: true,
            },
        )
        .unwrap();
        assert!(mag(&p, 440.0) < 0.005, "muted A must be silent");
        assert!(mag(&p, 880.0) > 0.05, "B still audible");
        apply(
            &mut p,
            &Command::SetTrackMute {
                track_id: ta,
                muted: false,
            },
        )
        .unwrap();
        apply(
            &mut p,
            &Command::SetTrackSolo {
                track_id: tb,
                solo: true,
            },
        )
        .unwrap();
        assert!(mag(&p, 440.0) < 0.005, "solo B must isolate");
        assert!(mag(&p, 880.0) > 0.05);
        apply(
            &mut p,
            &Command::SetTrackMute {
                track_id: tb,
                muted: true,
            },
        )
        .unwrap();
        assert!(mag(&p, 880.0) < 0.005, "mute wins over solo");
    }

    #[test]
    fn track_audibility_rules() {
        // Issue #39: 鳴らすトラックの判定（純粋関数）の真理値表。
        let mut p = Project::new("audible");
        let t = apply(
            &mut p,
            &Command::AddTrack {
                name: "A".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        let track = p.tracks[0].clone();
        assert!(is_track_audible(&track, false));
        assert!(
            !is_track_audible(&track, true),
            "ソロが他にある間はソロでないトラックは鳴らない"
        );
        apply(
            &mut p,
            &Command::SetTrackSolo {
                track_id: t,
                solo: true,
            },
        )
        .unwrap();
        assert!(is_track_audible(&p.tracks[0], true));
        apply(
            &mut p,
            &Command::SetTrackMute {
                track_id: t,
                muted: true,
            },
        )
        .unwrap();
        assert!(!is_track_audible(&p.tracks[0], true), "mute wins");
        assert!(!is_track_audible(&p.tracks[0], false));
    }

    #[test]
    fn render_mix_applies_clip_pitch() {
        // ピッチ付きクリップと無しのクリップが混ざっても破綻しない。
        let (mut p, audio) = project_with(&[("a.wav", 0, 1000, 1.0), ("b.wav", 0, 1000, 1.0)]);
        let clip_id = p.tracks[0].clips[0].id;
        apply(
            &mut p,
            &Command::SetClipPitch {
                clip_id,
                semitones: 12.0,
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 1000, RATE);
        let w = window(&mix, 2, 300, 700, RATE);
        // a.wav は 440→880 に上がり、b.wav の 880 と重なる。440 は消える。
        assert!(goertzel(&w, RATE, 880.0) > 0.15, "880 should dominate");
        assert!(goertzel(&w, RATE, 440.0) < 0.05, "440 should be gone");
    }
}

#[cfg(test)]
mod review_fix_tests {
    use super::tests::*;
    use super::*;
    use studio_core::{apply, Command, TrackKind};

    #[test]
    fn decode_24bit_wav_normalizes_by_bit_depth() {
        // Issue #13 レビュー指摘: 正規化はビット深度依存なので、
        // 16bit 以外（24bit）でも振幅が保たれることを固定する。
        let path = std::env::temp_dir().join(format!("daw-decode24-{}.wav", std::process::id()));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: RATE,
            bits_per_sample: 24,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        // hound の 24bit 書き込みは i32 の下位 24bit に値を置く
        for s in sine(440.0, 100, 0.5).samples {
            writer.write_sample((s * (1 << 23) as f32) as i32).unwrap();
        }
        writer.finalize().unwrap();
        let decoded = decode_audio(&path).unwrap();
        let max = decoded.samples.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
        assert!((max - 0.5).abs() < 0.02, "24bit amplitude was {max}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn render_mix_resamples_other_rates_without_pitch_change() {
        // Issue #13 レビュー指摘: ソースと出力のレートが違っても
        // 音程が保たれること（pos 計算がソースレート基準であること）。
        let mut p = Project::new("resample test");
        let track = apply(
            &mut p,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::AddClip {
                track_id: track,
                name: "low".into(),
                start_ms: 0,
                duration_ms: 1000,
                source: "low.wav".into(),
            },
        )
        .unwrap();
        // 22050Hz の 440Hz サイン
        let low_rate = 22050u32;
        let frames = low_rate as usize;
        let samples: Vec<f32> = (0..frames)
            .map(|i| 0.4 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / low_rate as f32).sin())
            .collect();
        let mut audio = HashMap::new();
        audio.insert(
            "low.wav".to_string(),
            ClipAudio {
                sample_rate: low_rate,
                channels: 1,
                samples,
            },
        );
        let mix = render_mix(&p, &audio, 0, 1000, RATE);
        let w = window(&mix, 2, 100, 900, RATE);
        assert!(
            goertzel(&w, RATE, 440.0) > 0.1,
            "440 must survive resampling"
        );
        assert!(goertzel(&w, RATE, 220.0) < 0.03);
        assert!(goertzel(&w, RATE, 880.0) < 0.03);
    }

    #[test]
    fn player_seek_clamps_above_project_end() {
        // Issue #13 レビュー指摘: seek の上限クランプを直接固定する。
        let mut player = Player::new(3000);
        player.seek(9999);
        assert_eq!(player.position_ms, 3000);
    }

    #[test]
    fn render_mix_matches_before_and_after_split() {
        // Issue #32: 分割前後でレンダリング結果が一致する（右クリップは
        // offset_ms 分だけ素材の途中から読まれる）。
        let (p, audio) = project_with(&[("a.wav", 0, 2000, 1.0)]);
        let before = render_mix(&p, &audio, 0, 2000, RATE);
        let clip_id = p.tracks[0].clips[0].id;
        let mut split = p.clone();
        apply(
            &mut split,
            &Command::SplitClip {
                clip_id,
                at_ms: 1000,
            },
        )
        .unwrap();
        let after = render_mix(&split, &audio, 0, 2000, RATE);
        assert_eq!(before.len(), after.len());
        let max_diff = before
            .iter()
            .zip(after.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max_diff < 1e-6, "split must not change the mix: {max_diff}");
    }

    #[test]
    fn render_mix_matches_after_duplicating_split_clip() {
        // Issue #62: 分割した右クリップを複製しても音が変わらない。
        // コピーが offset_ms を引き継ぐため、コピーの再生区間は元の
        // 右クリップの再生区間とサンプル単位で一致する。
        let (p, audio) = project_with(&[("a.wav", 0, 2000, 1.0)]);
        let clip_id = p.tracks[0].clips[0].id;
        let mut split = p.clone();
        let right_id = apply(
            &mut split,
            &Command::SplitClip {
                clip_id,
                at_ms: 1000,
            },
        )
        .unwrap()
        .unwrap();
        // 元の右クリップだけのミックス（1000〜2000ms）。
        let original = render_mix(&split, &audio, 1000, 1000, RATE);
        // DAW の配置計画で右クリップを複製する（終端 2000 に接する）。
        let placement = crate::edit::duplicate_placement(&split, right_id).expect("clip exists");
        let dup_id = apply(&mut split, &placement.add).unwrap().unwrap();
        for cmd in placement.follow_up_commands(dup_id) {
            apply(&mut split, &cmd).unwrap();
        }
        let (_, dup) = split.clip(dup_id).unwrap();
        assert_eq!(dup.offset_ms, 1000, "duplicate must keep the offset");
        // コピーだけのミックス（2000〜3000ms）が元と一致する。
        let copied = render_mix(&split, &audio, 2000, 1000, RATE);
        assert_eq!(original.len(), copied.len());
        let max_diff = original
            .iter()
            .zip(copied.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            max_diff < 1e-6,
            "duplicated split clip must sound the same: {max_diff}"
        );
    }

    #[test]
    fn render_mix_applies_linear_fade_in() {
        // Issue #34: フェードイン区間では振幅が直線的に増える。
        // 定数 0.8 の素材に fade_in 1000ms を付けると、クリップ内
        // 0ms で 0、500ms で 0.4、1000ms で 0.8 になる。
        let (mut p, _) = project_with(&[("a.wav", 0, 2000, 1.0)]);
        let clip_id = p.tracks[0].clips[0].id;
        // 定数素材に差し替える（サインでは直線性を判定しにくいため）
        let mut audio = HashMap::new();
        audio.insert(
            "a.wav".to_string(),
            ClipAudio {
                sample_rate: RATE,
                channels: 1,
                samples: vec![0.8; RATE as usize * 3],
            },
        );
        apply(
            &mut p,
            &Command::SetClipFade {
                clip_id,
                fade_in_ms: 1000,
                fade_out_ms: 0,
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 2000, RATE);
        let at = |ms: usize| mix[ms * RATE as usize / 1000 * 2]; // 左ch
        assert!(at(0).abs() < 0.01, "fade start must be silent: {}", at(0));
        assert!((at(250) - 0.2).abs() < 0.01, "250ms: {}", at(250));
        assert!((at(500) - 0.4).abs() < 0.01, "500ms: {}", at(500));
        assert!((at(1000) - 0.8).abs() < 0.01, "1000ms: {}", at(1000));
        assert!((at(1500) - 0.8).abs() < 0.01, "after fade: {}", at(1500));
    }

    #[test]
    fn render_mix_applies_linear_fade_out() {
        // Issue #34: フェードアウト区間では振幅が直線的に減る。
        let (mut p, _) = project_with(&[("a.wav", 0, 2000, 1.0)]);
        let clip_id = p.tracks[0].clips[0].id;
        let mut audio = HashMap::new();
        audio.insert(
            "a.wav".to_string(),
            ClipAudio {
                sample_rate: RATE,
                channels: 1,
                samples: vec![0.8; RATE as usize * 3],
            },
        );
        apply(
            &mut p,
            &Command::SetClipFade {
                clip_id,
                fade_in_ms: 0,
                fade_out_ms: 1000,
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 2000, RATE);
        let at = |ms: usize| mix[ms * RATE as usize / 1000 * 2];
        assert!((at(500) - 0.8).abs() < 0.01, "before fade: {}", at(500));
        assert!((at(1000) - 0.8).abs() < 0.01, "fade start: {}", at(1000));
        assert!((at(1500) - 0.4).abs() < 0.01, "1500ms: {}", at(1500));
        assert!(
            at(1999).abs() < 0.01,
            "fade end must be silent: {}",
            at(1999)
        );
    }

    #[test]
    fn exported_wav_round_trips_with_same_samples() {
        // Issue #41（イシュー指定名）: 書き出した WAV を読み戻すと
        // 44.1kHz / ステレオで、render_mix の出力と一致する
        // （16bit 量子化の誤差以内）。無音でもない。
        let (p, audio) = project_with(&[("a.wav", 0, 2_000, 1.0)]);
        let path = std::env::temp_dir().join(format!("daw-export-test-{}.wav", std::process::id()));
        let written = export_wav(&p, &audio, &path).unwrap();
        assert_eq!(written, path);
        let back = decode_audio(&path).unwrap();
        assert_eq!(back.sample_rate, 44_100);
        assert_eq!(back.channels, 2);
        let expected = render_mix(&p, &audio, 0, crate::timeline::project_end_ms(&p), 44_100);
        assert_eq!(back.samples.len(), expected.len());
        let tol = 2.0 / 32_767.0;
        for (a, b) in back.samples.iter().zip(expected.iter()) {
            assert!((a - b).abs() <= tol, "sample mismatch: {a} vs {b}");
        }
        assert!(rms(&back.samples) > 0.05, "export must not be silent");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn default_export_path_sits_next_to_project() {
        // Issue #41: 書き出し先の初期値はプロジェクトと同じ
        // ディレクトリの <プロジェクト名>.wav。
        let dir = std::path::Path::new("/tmp/somewhere/project.json");
        assert_eq!(
            default_export_path(Some(dir), "my song"),
            std::path::PathBuf::from("/tmp/somewhere/my song.wav")
        );
        assert_eq!(
            default_export_path(None, "my song"),
            std::path::PathBuf::from("my song.wav")
        );
    }

    #[test]
    fn metronome_clicks_land_on_beats() {
        // Issue #36: メトロノーム ON では拍の位置（bpm=120 で
        // 0, 500, 1000, 1500ms）にクリックが合成され、拍間は無音。
        // 小節頭（0ms）は 2000Hz、それ以外の拍は 1000Hz。
        let p = Project::new("metro");
        let audio = HashMap::new();
        let mix = render_mix_with_metronome(&p, &audio, 0, 2000, RATE, true);
        let peak_in = |from: i64, to: i64| {
            window(&mix, 2, from, to, RATE)
                .iter()
                .fold(0.0f32, |a, s| a.max(s.abs()))
        };
        for beat in [0, 500, 1000, 1500] {
            assert!(
                peak_in(beat, beat + 30) > 0.3,
                "click at {beat}ms: {}",
                peak_in(beat, beat + 30)
            );
        }
        assert!(peak_in(240, 260) < 0.01, "between beats must be silent");
        assert!(peak_in(740, 760) < 0.01, "between beats must be silent");
        // 強拍は 2000Hz、弱拍は 1000Hz が支配的
        let w0 = window(&mix, 2, 0, 25, RATE);
        assert!(
            goertzel(&w0, RATE, 2000.0) > goertzel(&w0, RATE, 1000.0),
            "bar head must be 2000Hz"
        );
        let w1 = window(&mix, 2, 500, 525, RATE);
        assert!(
            goertzel(&w1, RATE, 1000.0) > goertzel(&w1, RATE, 2000.0),
            "inner beat must be 1000Hz"
        );
    }

    #[test]
    fn metronome_off_adds_no_clicks() {
        // Issue #36: OFF ではクリックが混ざらない（空のプロジェクト
        // なのでミックスは完全な無音）。
        let p = Project::new("metro off");
        let audio = HashMap::new();
        let mix = render_mix_with_metronome(&p, &audio, 0, 2000, RATE, false);
        assert!(mix.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn metronome_interval_follows_bpm() {
        // Issue #36: BPM を変えるとクリック間隔が追随する
        // （bpm=60 では 1000ms 間隔で、500ms 地点には鳴らない）。
        let mut p = Project::new("metro 60");
        apply(&mut p, &Command::SetBpm { bpm: 60.0 }).unwrap();
        let audio = HashMap::new();
        let mix = render_mix_with_metronome(&p, &audio, 0, 2000, RATE, true);
        let peak_in = |from: i64, to: i64| {
            window(&mix, 2, from, to, RATE)
                .iter()
                .fold(0.0f32, |a, s| a.max(s.abs()))
        };
        assert!(peak_in(0, 30) > 0.3);
        assert!(peak_in(1000, 1030) > 0.3);
        assert!(peak_in(490, 510) < 0.01, "no click at 500ms when bpm=60");
    }
}

#[cfg(test)]
mod note_render_tests {
    use super::tests::*;
    use super::*;
    use studio_core::{apply, Command, Instrument, TrackKind};

    /// ノートクリップとサンプラー楽器を持つプロジェクト。
    /// 素材は 220Hz サイン（tone.wav）と比較用の 330Hz（other.wav）。
    fn project_with_melody() -> (Project, HashMap<String, ClipAudio>, u64, u64) {
        let mut p = Project::new("melody");
        let track = apply(
            &mut p,
            &Command::AddTrack {
                name: "A1".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::SetTrackInstrument {
                track_id: track,
                instrument: Some(Instrument::Sampler {
                    source: "tone.wav".into(),
                    root_note: 60,
                }),
            },
        )
        .unwrap();
        let clip = apply(
            &mut p,
            &Command::AddClip {
                track_id: track,
                name: "melody".into(),
                start_ms: 0,
                duration_ms: 2_000,
                source: String::new(),
            },
        )
        .unwrap()
        .unwrap();
        let mut audio = HashMap::new();
        audio.insert("tone.wav".to_string(), sine(220.0, 3_000, 0.4));
        audio.insert("other.wav".to_string(), sine(330.0, 3_000, 0.4));
        (p, audio, track, clip)
    }

    fn add_note(p: &mut Project, clip: u64, pitch: u8, start: i64, dur: i64, vel: u8) {
        apply(
            p,
            &Command::AddNote {
                clip_id: clip,
                pitch,
                start_ms: start,
                duration_ms: dur,
                velocity: vel,
            },
        )
        .unwrap();
    }

    /// 50ms 窓の Goertzel 平均（pitch_tests と同じ測定法）。
    /// ピッチ変更後の信号は周波数の微小な誤差で長い窓だと位相が
    /// 崩れて過小評価になるため、短い窓の平均で測る。
    fn segmented_goertzel(samples: &[f32], rate: u32, freq: f32) -> f32 {
        let seg = rate as usize / 20;
        let mut total = 0.0f32;
        let mut count = 0usize;
        for chunk in samples.chunks(seg) {
            if chunk.len() == seg {
                total += goertzel(chunk, rate, freq);
                count += 1;
            }
        }
        total / count.max(1) as f32
    }

    #[test]
    fn render_mix_plays_notes_with_sampler() {
        // Issue #68: ノートの区間だけ、素材の音が鳴る。前後は無音。
        let (mut p, audio, _, clip) = project_with_melody();
        add_note(&mut p, clip, 60, 500, 500, 127);
        let mix = render_mix(&p, &audio, 0, 2_000, RATE);
        let before = window(&mix, 2, 0, 400, RATE);
        assert!(rms(&before) < 1e-4, "before the note must be silent");
        let during = window(&mix, 2, 600, 900, RATE);
        assert!(
            goertzel(&during, RATE, 220.0) > 0.1,
            "root note must sound at the source pitch"
        );
        let after = window(&mix, 2, 1_100, 1_900, RATE);
        assert!(rms(&after) < 1e-4, "after the note must be silent");
    }

    #[test]
    fn render_mix_note_pitch_follows_instrument_root() {
        // Issue #68: 音高シフト量は note.pitch - instrument.root_note。
        // 同じノート（60）でもルートを 48 にすると +12 半音で鳴る。
        let (mut p, audio, track, clip) = project_with_melody();
        add_note(&mut p, clip, 60, 0, 1_000, 127);
        let mix = render_mix(&p, &audio, 0, 1_000, RATE);
        let w = window(&mix, 2, 300, 700, RATE);
        assert!(goertzel(&w, RATE, 220.0) > 0.1, "root 60: 220Hz");
        assert!(goertzel(&w, RATE, 440.0) < 0.03);
        apply(
            &mut p,
            &Command::SetTrackInstrument {
                track_id: track,
                instrument: Some(Instrument::Sampler {
                    source: "tone.wav".into(),
                    root_note: 48,
                }),
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 1_000, RATE);
        let w = window(&mix, 2, 300, 700, RATE);
        assert!(
            segmented_goertzel(&w, RATE, 440.0) > 0.05,
            "root 48: +12 -> 440Hz"
        );
        assert!(segmented_goertzel(&w, RATE, 220.0) < 0.01);
    }

    #[test]
    fn render_mix_switching_tone_changes_the_sound() {
        // Issue #73: 音色を割り当て・切り替えると再生される音が
        // 変わる。音色の中身の差し替え（UpdateTone）も参照経由で
        // 即座に反映される。
        let (mut p, audio, track, clip) = project_with_melody();
        add_note(&mut p, clip, 60, 0, 1_000, 127);
        let tone_a = apply(
            &mut p,
            &Command::AddTone {
                name: "A".into(),
                instrument: Instrument::Sampler {
                    source: "tone.wav".into(),
                    root_note: 60,
                },
            },
        )
        .unwrap()
        .unwrap();
        let tone_b = apply(
            &mut p,
            &Command::AddTone {
                name: "B".into(),
                instrument: Instrument::Sampler {
                    source: "tone.wav".into(),
                    root_note: 48,
                },
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::SetTrackTone {
                track_id: track,
                tone_id: Some(tone_a),
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 1_000, RATE);
        let w = window(&mix, 2, 300, 700, RATE);
        assert!(goertzel(&w, RATE, 220.0) > 0.1, "tone A (root 60): 220Hz");
        apply(
            &mut p,
            &Command::SetTrackTone {
                track_id: track,
                tone_id: Some(tone_b),
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 1_000, RATE);
        let w = window(&mix, 2, 300, 700, RATE);
        assert!(
            segmented_goertzel(&w, RATE, 440.0) > 0.05,
            "tone B (root 48): +12 -> 440Hz"
        );
        assert!(segmented_goertzel(&w, RATE, 220.0) < 0.01);
        apply(
            &mut p,
            &Command::UpdateTone {
                tone_id: tone_b,
                instrument: Instrument::Sampler {
                    source: "tone.wav".into(),
                    root_note: 60,
                },
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 1_000, RATE);
        let w = window(&mix, 2, 300, 700, RATE);
        assert!(goertzel(&w, RATE, 220.0) > 0.1, "updated tone B: 220Hz");
    }

    #[test]
    fn render_mix_swapping_instrument_changes_the_sound() {
        // Issue #68: 同じノート列でも、instrument の素材を差し替えると
        // 別の素材音で鳴る（音色とメロディーの分離）。
        let (mut p, audio, track, clip) = project_with_melody();
        add_note(&mut p, clip, 60, 0, 1_000, 127);
        let mix_a = render_mix(&p, &audio, 0, 1_000, RATE);
        apply(
            &mut p,
            &Command::SetTrackInstrument {
                track_id: track,
                instrument: Some(Instrument::Sampler {
                    source: "other.wav".into(),
                    root_note: 60,
                }),
            },
        )
        .unwrap();
        let mix_b = render_mix(&p, &audio, 0, 1_000, RATE);
        let wa = window(&mix_a, 2, 300, 700, RATE);
        let wb = window(&mix_b, 2, 300, 700, RATE);
        assert!(goertzel(&wa, RATE, 220.0) > 0.1);
        assert!(goertzel(&wa, RATE, 330.0) < 0.03);
        assert!(goertzel(&wb, RATE, 330.0) > 0.1);
        assert!(goertzel(&wb, RATE, 220.0) < 0.03);
    }

    #[test]
    fn render_mix_note_velocity_scales_gain() {
        // Issue #68: ベロシティは 0–127 を 0.0–1.0 のゲインに線形変換。
        let (mut p, audio, _, clip) = project_with_melody();
        add_note(&mut p, clip, 60, 0, 1_000, 127);
        let loud = render_mix(&p, &audio, 0, 1_000, RATE);
        let (mut p2, _, _, clip2) = project_with_melody();
        add_note(&mut p2, clip2, 60, 0, 1_000, 64);
        let quiet = render_mix(&p2, &audio, 0, 1_000, RATE);
        let ratio =
            rms(&window(&quiet, 2, 100, 900, RATE)) / rms(&window(&loud, 2, 100, 900, RATE));
        assert!(
            (ratio - 64.0 / 127.0).abs() < 0.03,
            "velocity ratio was {ratio}"
        );
    }

    #[test]
    fn render_mix_notes_silent_without_instrument_or_when_muted() {
        // Issue #68: 楽器のないトラックのノートは鳴らない。
        // ミュート（#39）の規則も波形クリップと同じに効く。
        let (mut p, audio, track, clip) = project_with_melody();
        add_note(&mut p, clip, 60, 0, 1_000, 127);
        apply(
            &mut p,
            &Command::SetTrackInstrument {
                track_id: track,
                instrument: None,
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 1_000, RATE);
        assert!(
            rms(&window(&mix, 2, 100, 900, RATE)) < 1e-4,
            "no instrument -> silent"
        );
        apply(
            &mut p,
            &Command::SetTrackInstrument {
                track_id: track,
                instrument: Some(Instrument::Sampler {
                    source: "tone.wav".into(),
                    root_note: 60,
                }),
            },
        )
        .unwrap();
        apply(
            &mut p,
            &Command::SetTrackMute {
                track_id: track,
                muted: true,
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 1_000, RATE);
        assert!(
            rms(&window(&mix, 2, 100, 900, RATE)) < 1e-4,
            "muted -> silent"
        );
    }

    #[test]
    fn export_wav_contains_melody() {
        // Issue #68: WAV 書き出し（#41 の経路）でも再生と同じ
        // メロディーが書き出される。
        let (mut p, audio, _, clip) = project_with_melody();
        add_note(&mut p, clip, 60, 0, 1_000, 127);
        add_note(&mut p, clip, 72, 1_000, 1_000, 127);
        let path =
            std::env::temp_dir().join(format!("daw-export-melody-{}.wav", std::process::id()));
        export_wav(&p, &audio, &path).unwrap();
        let back = decode_audio(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let first = window(&back.samples, 2, 300, 700, back.sample_rate);
        assert!(
            goertzel(&first, back.sample_rate, 220.0) > 0.1,
            "first note (root) must be in the export"
        );
        let second = window(&back.samples, 2, 1_300, 1_700, back.sample_rate);
        assert!(
            segmented_goertzel(&second, back.sample_rate, 440.0) > 0.05,
            "second note (+12) must be in the export"
        );
    }
}

#[cfg(test)]
mod note_solo_tests {
    use super::tests::*;
    use super::*;
    use studio_core::{apply, Command, Instrument, TrackKind};

    #[test]
    fn render_mix_note_track_respects_solo() {
        // レビュー Minor-2（Issue #68）: ソロ（#39）の規則もノートに
        // 波形クリップと同じに効く。ソロしたトラックのノートだけが
        // 鳴り、非ソロ側のノートは鳴らない。
        let mut p = Project::new("solo-notes");
        let mut audio = HashMap::new();
        audio.insert("tone.wav".to_string(), sine(220.0, 3_000, 0.4));
        audio.insert("other.wav".to_string(), sine(330.0, 3_000, 0.4));
        let mut track_ids = Vec::new();
        for source in ["tone.wav", "other.wav"] {
            let track = apply(
                &mut p,
                &Command::AddTrack {
                    name: source.into(),
                    kind: TrackKind::Audio,
                },
            )
            .unwrap()
            .unwrap();
            apply(
                &mut p,
                &Command::SetTrackInstrument {
                    track_id: track,
                    instrument: Some(Instrument::Sampler {
                        source: source.into(),
                        root_note: 60,
                    }),
                },
            )
            .unwrap();
            let clip = apply(
                &mut p,
                &Command::AddClip {
                    track_id: track,
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
                    clip_id: clip,
                    pitch: 60,
                    start_ms: 0,
                    duration_ms: 1_000,
                    velocity: 127,
                },
            )
            .unwrap();
            track_ids.push(track);
        }
        // ソロなしでは両方のノートが鳴る
        let mix = render_mix(&p, &audio, 0, 1_000, RATE);
        let w = window(&mix, 2, 300, 700, RATE);
        assert!(goertzel(&w, RATE, 220.0) > 0.05);
        assert!(goertzel(&w, RATE, 330.0) > 0.05);
        // other.wav のトラックをソロにすると 330Hz だけが残る
        apply(
            &mut p,
            &Command::SetTrackSolo {
                track_id: track_ids[1],
                solo: true,
            },
        )
        .unwrap();
        let mix = render_mix(&p, &audio, 0, 1_000, RATE);
        let w = window(&mix, 2, 300, 700, RATE);
        assert!(goertzel(&w, RATE, 330.0) > 0.05, "solo track must sound");
        assert!(
            goertzel(&w, RATE, 220.0) < 0.01,
            "non-solo track must be silent"
        );
    }
}

#[cfg(test)]
mod midi_import_tests {
    use super::tests::*;
    use super::*;
    use studio_core::{import_midi, Command, History, Instrument};

    /// 50ms 窓の Goertzel 平均（note_render_tests と同じ測定法）。
    fn segmented_goertzel(samples: &[f32], rate: u32, freq: f32) -> f32 {
        let seg = rate as usize / 20;
        let mut total = 0.0f32;
        let mut count = 0usize;
        for chunk in samples.chunks(seg) {
            if chunk.len() == seg {
                total += goertzel(chunk, rate, freq);
                count += 1;
            }
        }
        total / count.max(1) as f32
    }

    /// 検証用 SMF（format 0、TPQN 480、120 BPM、ノート 60 を
    /// 0–480 tick、ノート 72 を 480–960 tick）。
    fn test_smf() -> Vec<u8> {
        fn vlq(mut n: u32) -> Vec<u8> {
            let mut buf = vec![(n & 0x7f) as u8];
            n >>= 7;
            while n > 0 {
                buf.push(((n & 0x7f) | 0x80) as u8);
                n >>= 7;
            }
            buf.reverse();
            buf
        }
        let mut t = Vec::new();
        t.extend(vlq(0));
        t.extend([0xff, 0x51, 0x03, 0x07, 0xa1, 0x20]);
        t.extend(vlq(0));
        t.extend([0x90, 60, 100]);
        t.extend(vlq(480));
        t.extend([0x80, 60, 64]);
        t.extend(vlq(0));
        t.extend([0x90, 72, 100]);
        t.extend(vlq(480));
        t.extend([0x80, 72, 64]);
        t.extend(vlq(0));
        t.extend([0xff, 0x2f, 0x00]);
        let mut smf = b"MThd".to_vec();
        smf.extend(6u32.to_be_bytes());
        smf.extend(0u16.to_be_bytes());
        smf.extend(1u16.to_be_bytes());
        smf.extend(480u16.to_be_bytes());
        smf.extend(b"MTrk");
        smf.extend((t.len() as u32).to_be_bytes());
        smf.extend(t);
        smf
    }

    #[test]
    fn imported_melody_plays_through_the_sampler_path() {
        // Issue #70: 取り込んだノート列に楽器を割り当てると、
        // #68 の再生経路で鳴る（素材 220Hz、ルート 60 で
        // ノート 60 → 220Hz、ノート 72 → 440Hz）。
        let mut history = History::new(Project::new("midi"));
        let summary = import_midi(&mut history, &test_smf()).unwrap();
        assert_eq!(summary.notes_added, 2);
        let project = history.project().clone();
        let track_id = project.tracks[0].id;
        let mut project = project;
        studio_core::apply(
            &mut project,
            &Command::SetTrackInstrument {
                track_id,
                instrument: Some(Instrument::Sampler {
                    source: "tone.wav".into(),
                    root_note: 60,
                }),
            },
        )
        .unwrap();
        let mut audio = HashMap::new();
        audio.insert("tone.wav".to_string(), sine(220.0, 3_000, 0.4));
        let mix = render_mix(&project, &audio, 0, 1_000, RATE);
        let first = window(&mix, 2, 100, 400, RATE);
        assert!(goertzel(&first, RATE, 220.0) > 0.1, "note 60 sounds");
        let second = window(&mix, 2, 600, 900, RATE);
        assert!(
            segmented_goertzel(&second, RATE, 440.0) > 0.05,
            "note 72 sounds an octave up"
        );
    }
}

#[cfg(test)]
mod plugin_render_tests {
    use super::tests::*;
    use super::*;
    use studio_core::{apply, Command, Instrument, PluginFormat, TrackKind};

    /// Plugin 楽器のノートクリップを持つプロジェクト。
    fn project_with_plugin_track() -> (Project, u64, u64) {
        let mut p = Project::new("plugin");
        let track = apply(
            &mut p,
            &Command::AddTrack {
                name: "VITAL".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::SetTrackInstrument {
                track_id: track,
                instrument: Some(Instrument::Plugin {
                    format: PluginFormat::Clap,
                    plugin_path: "/plugins/Vital.clap".into(),
                    preset_path: None,
                }),
            },
        )
        .unwrap();
        let clip = apply(
            &mut p,
            &Command::AddClip {
                track_id: track,
                name: "melody".into(),
                start_ms: 500,
                duration_ms: 1_000,
                source: String::new(),
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::AddNote {
                clip_id: clip,
                pitch: 69,
                start_ms: 0,
                duration_ms: 1_000,
                velocity: 127,
            },
        )
        .unwrap();
        (p, track, clip)
    }

    #[test]
    fn render_mix_plays_plugin_track_from_prerendered_audio() {
        // Issue #72: プラグイントラックは事前レンダリング済み音声
        // （絶対時刻で用意される）をクリップの区間だけ鳴らす。
        let (p, track, _clip) = project_with_plugin_track();
        let mut audio = HashMap::new();
        audio.insert(plugin_track_key(track), sine(440.0, 3_000, 0.4));
        let mix = render_mix(&p, &audio, 0, 2_000, RATE);
        let before = window(&mix, 2, 0, 400, RATE);
        assert!(rms(&before) < 1e-4, "before the clip must be silent");
        let during = window(&mix, 2, 600, 1_400, RATE);
        assert!(
            goertzel(&during, RATE, 440.0) > 0.1,
            "plugin audio must sound inside the clip"
        );
        let after = window(&mix, 2, 1_600, 1_900, RATE);
        assert!(rms(&after) < 1e-4, "after the clip must be silent");
    }

    #[test]
    fn render_mix_plugin_track_without_prerender_stays_silent_and_keeps_sampler() {
        // 事前レンダリングが無い（プラグイン不在等）プラグイン
        // トラックは無音になるが、他のサンプラートラックは壊れない。
        let (mut p, _track, _clip) = project_with_plugin_track();
        let sampler_track = apply(
            &mut p,
            &Command::AddTrack {
                name: "SAMPLER".into(),
                kind: TrackKind::Audio,
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::SetTrackInstrument {
                track_id: sampler_track,
                instrument: Some(Instrument::Sampler {
                    source: "tone.wav".into(),
                    root_note: 60,
                }),
            },
        )
        .unwrap();
        let sampler_clip = apply(
            &mut p,
            &Command::AddClip {
                track_id: sampler_track,
                name: "melody2".into(),
                start_ms: 500,
                duration_ms: 1_000,
                source: String::new(),
            },
        )
        .unwrap()
        .unwrap();
        apply(
            &mut p,
            &Command::AddNote {
                clip_id: sampler_clip,
                pitch: 60,
                start_ms: 0,
                duration_ms: 1_000,
                velocity: 127,
            },
        )
        .unwrap();
        let mut audio = HashMap::new();
        audio.insert("tone.wav".to_string(), sine(220.0, 3_000, 0.4));
        let mix = render_mix(&p, &audio, 0, 2_000, RATE);
        let during = window(&mix, 2, 600, 1_400, RATE);
        assert!(
            goertzel(&during, RATE, 220.0) > 0.1,
            "sampler track must keep sounding"
        );
        assert!(
            goertzel(&during, RATE, 440.0) < 0.03,
            "plugin track must not sound without prerender"
        );
    }
}
