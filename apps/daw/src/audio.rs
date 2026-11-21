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

/// プロジェクトのミックスを指定範囲でレンダリングする（ステレオ出力）。
/// 配置位置・ゲインはプロジェクトの状態どおりに反映される。
pub fn render_mix(
    project: &Project,
    audio: &HashMap<String, ClipAudio>,
    start_ms: i64,
    duration_ms: i64,
    out_rate: u32,
) -> Vec<f32> {
    let frames = (duration_ms.max(0) as u64 * out_rate as u64 / 1000) as usize;
    let mut out = vec![0.0f32; frames * 2];
    // ピッチ変更済みクリップ音声のキャッシュ（このレンダリング内のみ）。
    let mut pitch_cache: HashMap<(String, u32), ClipAudio> = HashMap::new();
    for (i, frame) in out.chunks_mut(2).enumerate() {
        // 時刻は f64 で持つ。整数ミリ秒に丸めると1ms ごとに波形が階段状に
        // なり、音程が壊れる（Issue #13 のテストで検出）。
        let t_ms = start_ms as f64 + i as f64 * 1000.0 / out_rate as f64;
        for track in &project.tracks {
            if track.kind != studio_core::TrackKind::Audio {
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
                // クリップ内位置 → ソースのフレーム位置（線形補間）
                let pos = (t_ms - clip.start_ms as f64) * clip_audio.sample_rate as f64 / 1000.0;
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
                    *slot += (a + (b - a) * frac) * clip.gain * channel_factor[out_ch];
                }
            }
        }
        for slot in frame.iter_mut() {
            *slot = slot.clamp(-1.0, 1.0);
        }
    }
    out
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
    use studio_core::{apply, Command};

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
        let path =
            std::env::temp_dir().join(format!("daw-decode24-{}.wav", std::process::id()));
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
}
