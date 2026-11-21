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
    for (i, frame) in out.chunks_mut(2).enumerate() {
        // 時刻は f64 で持つ。整数ミリ秒に丸めると1ms ごとに波形が階段状に
        // なり、音程が壊れる（Issue #13 のテストで検出）。
        let t_ms = start_ms as f64 + i as f64 * 1000.0 / out_rate as f64;
        for track in &project.tracks {
            if track.kind != studio_core::TrackKind::Audio {
                continue;
            }
            for clip in &track.clips {
                if t_ms < clip.start_ms as f64 || t_ms >= (clip.start_ms + clip.duration_ms) as f64
                {
                    continue;
                }
                let Some(clip_audio) = audio.get(&clip.source) else {
                    continue;
                };
                if clip_audio.channels == 0 || clip_audio.sample_rate == 0 {
                    continue;
                }
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
                    *slot += (a + (b - a) * frac) * clip.gain;
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

    const RATE: u32 = 44100;

    fn sine(freq: f32, ms: i64, amp: f32) -> ClipAudio {
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

    fn goertzel(samples: &[f32], rate: u32, freq: f32) -> f32 {
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

    fn window(samples: &[f32], channels: usize, from_ms: i64, to_ms: i64, rate: u32) -> Vec<f32> {
        let a = (from_ms * rate as i64 / 1000) as usize * channels;
        let b = (to_ms * rate as i64 / 1000) as usize * channels;
        // 左チャンネルだけ取り出す
        samples[a..b].iter().step_by(channels).cloned().collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    fn project_with(clips: &[(&str, i64, i64, f32)]) -> (Project, HashMap<String, ClipAudio>) {
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
