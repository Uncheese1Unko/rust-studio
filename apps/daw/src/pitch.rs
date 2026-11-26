//! 単旋律ピッチ追跡とノート化（Issue #100）。
//!
//! #98 の調査試作（自己相関、外部クレートなし）を土台に、
//! デコード済み音声（[`crate::audio::ClipAudio`]）から
//! ノート列を抽出する。重い処理はアプリ側に置き、コアには
//! ノート列という結果だけをコマンドで渡す（architecture ルール）。
//!
//! 前提は単旋律のみ。和音・複数音の同時追跡はしない。
//! 音高の定まらない区間（無音、ドラム等の非周期音）は
//! ノートを作らず、正直に空として扱う。

use crate::audio::ClipAudio;

/// 分析窓の長さ（サンプル）。
pub const WINDOW: usize = 2048;
/// フレーム間隔（サンプル）。44.1kHz で約 11.6ms。
pub const HOP: usize = 512;
/// 探索する基本周波数の下限（Hz）。
pub const MIN_FREQ: f32 = 50.0;
/// 探索する基本周波数の上限（Hz）。
pub const MAX_FREQ: f32 = 1200.0;
/// これ未満のフレーム RMS は無音として音高なしにする。
pub const RMS_THRESHOLD: f32 = 0.02;
/// 正規化自己相関の最大値がこれ未満なら周期性なし（音高なし）。
pub const CORR_THRESHOLD: f32 = 0.5;
/// ノートとして成立する最小の連続フレーム数。
pub const MIN_NOTE_FRAMES: usize = 4;
/// 同一音高の区間内で吸収する無音高フレーム数。
pub const GAP_FRAMES: usize = 1;

/// 抽出されたノート。モデルの `Note` と同じ形で、開始・長さは
/// クリップ相対のミリ秒。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractedNote {
    pub pitch: u8,
    pub start_ms: i64,
    pub duration_ms: i64,
    pub velocity: u8,
}

/// 音声からノート列を抽出する。決定的（同じ入力→同じ出力）。
pub fn extract_notes(audio: &ClipAudio) -> Vec<ExtractedNote> {
    let mono = mix_down(audio);
    let sr = audio.sample_rate as f32;
    if sr <= 0.0 || mono.len() < WINDOW {
        return Vec::new();
    }
    // フレームごとの (音高, RMS)。音高なしは None。
    let mut frames: Vec<(Option<u8>, f32)> = Vec::new();
    let mut pos = 0usize;
    while pos + WINDOW <= mono.len() {
        let frame = &mono[pos..pos + WINDOW];
        let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
        let pitch = estimate_pitch(frame, sr).map(freq_to_midi);
        frames.push((pitch, rms));
        pos += HOP;
    }
    let hop_ms = HOP as f64 / sr as f64 * 1000.0;
    let total_ms = audio.duration_ms();
    // 区間化: 同一音高の連続をノートにする（GAP_FRAMES までの
    // 音高なしは吸収する）。#98 試作と同じ規則。
    let mut notes = Vec::new();
    let mut i = 0usize;
    while i < frames.len() {
        let Some(p) = frames[i].0 else {
            i += 1;
            continue;
        };
        let mut j = i;
        let mut last = i;
        while j < frames.len() {
            match frames[j].0 {
                Some(q) if q == p => {
                    last = j;
                    j += 1;
                }
                None if j - last <= GAP_FRAMES => j += 1,
                _ => break,
            }
        }
        let len = last - i + 1;
        if len >= MIN_NOTE_FRAMES {
            let peak_rms = frames[i..=last].iter().map(|f| f.1).fold(0.0f32, f32::max);
            let velocity = ((peak_rms / 0.5) * 127.0).round().clamp(1.0, 127.0) as u8;
            let start_ms = (i as f64 * hop_ms) as i64;
            let end_ms = ((start_ms as f64 + len as f64 * hop_ms) as i64).min(total_ms);
            if end_ms > start_ms {
                notes.push(ExtractedNote {
                    pitch: p,
                    start_ms,
                    duration_ms: end_ms - start_ms,
                    velocity,
                });
            }
        }
        i = j.max(i + 1);
    }
    notes
}

/// チャンネルを平均してモノラル化する。
fn mix_down(audio: &ClipAudio) -> Vec<f32> {
    let ch = audio.channels.max(1) as usize;
    audio
        .samples
        .chunks_exact(ch)
        .map(|f| f.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// 自己相関による基本周波数推定。無音／周期性なしは None。
fn estimate_pitch(frame: &[f32], sample_rate: f32) -> Option<f32> {
    let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
    if rms < RMS_THRESHOLD {
        return None;
    }
    let e0: f32 = frame.iter().map(|s| s * s).sum();
    if e0 <= 0.0 {
        return None;
    }
    let min_lag = (sample_rate / MAX_FREQ) as usize;
    let max_lag = ((sample_rate / MIN_FREQ) as usize).min(frame.len() / 2);
    let mut best_lag = 0usize;
    let mut best_corr = 0.0f32;
    for lag in min_lag..=max_lag {
        let mut corr = 0.0f32;
        for i in 0..frame.len() - lag {
            corr += frame[i] * frame[i + lag];
        }
        let norm = corr / e0;
        if norm > best_corr {
            best_corr = norm;
            best_lag = lag;
        }
    }
    if best_lag == 0 || best_corr < CORR_THRESHOLD {
        return None;
    }
    Some(sample_rate / best_lag as f32)
}

fn freq_to_midi(freq: f32) -> u8 {
    (69.0 + 12.0 * (freq / 440.0).log2())
        .round()
        .clamp(0.0, 127.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 44100;

    fn mono_audio(samples: Vec<f32>) -> ClipAudio {
        ClipAudio {
            sample_rate: SR,
            channels: 1,
            samples,
        }
    }

    /// 減衰エンベロープ付きの正弦波（倍音つき）。#98 のテスト WAV と
    /// 同じ作り方で、実音に近い周期波形にする。
    fn tone(freq: f32, seconds: f32, amp: f32) -> Vec<f32> {
        let n = (SR as f32 * seconds) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / SR as f32;
                let env = (-3.0 * t / seconds).exp();
                let ph = 2.0 * std::f32::consts::PI * freq * t;
                amp * env * (ph.sin() + 0.5 * (2.0 * ph).sin() + 0.25 * (3.0 * ph).sin())
            })
            .collect()
    }

    fn melody() -> ClipAudio {
        let mut samples = Vec::new();
        for freq in [440.0, 523.25, 659.26, 880.0] {
            samples.extend(tone(freq, 0.5, 0.35));
        }
        mono_audio(samples)
    }

    #[test]
    fn extracts_known_melody_pitches_and_onsets() {
        let notes = extract_notes(&melody());
        let pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
        assert_eq!(pitches, vec![69, 72, 76, 81], "{notes:?}");
        for (note, expected_start) in notes.iter().zip([0, 500, 1000, 1500]) {
            assert!(
                (note.start_ms - expected_start).abs() <= 100,
                "onset {} vs expected {expected_start}",
                note.start_ms
            );
            assert!(
                (note.duration_ms - 500).abs() <= 200,
                "duration {} vs expected ~500",
                note.duration_ms
            );
        }
    }

    #[test]
    fn silence_yields_no_notes() {
        let audio = mono_audio(vec![0.0; SR as usize]);
        assert!(extract_notes(&audio).is_empty());
    }

    #[test]
    fn noise_yields_no_notes() {
        // 決定的な疑似ノイズ（線形合同法）。周期性がないので
        // ノートは生成されないのが正しい結果。
        let mut state: u32 = 0x12345678;
        let samples: Vec<f32> = (0..SR as usize)
            .map(|_| {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                (state >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
            })
            .collect();
        let audio = mono_audio(samples);
        assert!(extract_notes(&audio).is_empty());
    }

    #[test]
    fn extraction_is_deterministic() {
        let audio = melody();
        assert_eq!(extract_notes(&audio), extract_notes(&audio));
    }

    #[test]
    fn notes_satisfy_model_invariants() {
        let audio = melody();
        let total_ms = audio.duration_ms();
        for note in extract_notes(&audio) {
            assert!(note.pitch <= 127);
            assert!((1..=127).contains(&note.velocity));
            assert!(note.start_ms >= 0);
            assert!(note.duration_ms > 0);
            assert!(note.start_ms + note.duration_ms <= total_ms, "{note:?}");
        }
    }

    #[test]
    fn stereo_input_is_mixed_down() {
        // 左右同信号のステレオでもモノラルと同じ結果になる。
        let mono = melody();
        let mut stereo_samples = Vec::new();
        for s in &mono.samples {
            stereo_samples.push(*s);
            stereo_samples.push(*s);
        }
        let stereo = ClipAudio {
            sample_rate: SR,
            channels: 2,
            samples: stereo_samples,
        };
        assert_eq!(extract_notes(&stereo), extract_notes(&mono));
    }
}
