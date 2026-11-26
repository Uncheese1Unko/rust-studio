// 調査試作（Issue #98）: 単旋律ピッチ追跡。
// 自己相関で フレームごとの基本周波数を推定し、MIDI ノート列へ区間化する。
// 外部クレートなし。入力は PCM16 WAV（モノラル化して処理）。

use std::env;
use std::fs;

fn read_wav_pcm16(path: &str) -> (Vec<f32>, u32) {
    let data = fs::read(path).expect("read wav");
    assert_eq!(&data[0..4], b"RIFF");
    let mut pos = 12usize;
    let mut sample_rate = 44100u32;
    let mut channels = 1u16;
    let mut samples: Vec<f32> = Vec::new();
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &data[pos + 8..(pos + 8 + size).min(data.len())];
        match id {
            b"fmt " => {
                channels = u16::from_le_bytes(body[2..4].try_into().unwrap());
                sample_rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
            }
            b"data" => {
                for pair in body.chunks_exact(2) {
                    samples.push(i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0);
                }
            }
            _ => {}
        }
        pos += 8 + size + (size % 2);
    }
    // モノラル化
    let ch = channels.max(1) as usize;
    let mono: Vec<f32> = samples
        .chunks_exact(ch)
        .map(|f| f.iter().sum::<f32>() / ch as f32)
        .collect();
    (mono, sample_rate)
}

/// 自己相関による基本周波数推定。無音／周期性なしは None。
fn estimate_pitch(frame: &[f32], sample_rate: f32, min_freq: f32, max_freq: f32) -> Option<f32> {
    let rms = (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt();
    if rms < 0.02 {
        return None;
    }
    let min_lag = (sample_rate / max_freq) as usize;
    let max_lag = ((sample_rate / min_freq) as usize).min(frame.len() / 2);
    let mut best_lag = 0usize;
    let mut best_corr = 0.0f32;
    // 正規化自己相関（ラグ 0 のエネルギーで割る）
    let e0: f32 = frame.iter().map(|s| s * s).sum();
    if e0 <= 0.0 {
        return None;
    }
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
    if best_lag == 0 || best_corr < 0.5 {
        return None;
    }
    // 放物線補間でラグを精密化
    let lag_f = best_lag as f32;
    Some(sample_rate / lag_f)
}

fn freq_to_midi(freq: f32) -> u8 {
    (69.0 + 12.0 * (freq / 440.0).log2()).round() as u8
}

#[derive(Debug, Clone, Copy)]
struct NoteSeg {
    pitch: u8,
    start_ms: i64,
    duration_ms: i64,
}

fn track(samples: &[f32], sample_rate: u32) -> Vec<NoteSeg> {
    let sr = sample_rate as f32;
    let win = 2048usize;
    let hop = 512usize;
    let hop_ms = hop as f64 / sr as f64 * 1000.0;
    // フレームごとの (pitch or None, rms)
    let mut frames: Vec<Option<u8>> = Vec::new();
    let mut pos = 0usize;
    while pos + win <= samples.len() {
        let frame = &samples[pos..pos + win];
        let pitch = estimate_pitch(frame, sr, 50.0, 1200.0).map(freq_to_midi);
        frames.push(pitch);
        pos += hop;
    }
    // 区間化: 同一ピッチの連続をノートにする（1フレームの欠落は許容）
    let min_frames = 4usize; // ≒ 46ms + 窓。最小ノート長の目安
    let mut notes = Vec::new();
    let mut i = 0usize;
    while i < frames.len() {
        match frames[i] {
            None => i += 1,
            Some(p) => {
                let mut j = i;
                let mut last = i;
                while j < frames.len() {
                    match frames[j] {
                        Some(q) if q == p => {
                            last = j;
                            j += 1;
                        }
                        None if j - last <= 1 => j += 1,
                        _ => break,
                    }
                }
                let len = last - i + 1;
                if len >= min_frames {
                    notes.push(NoteSeg {
                        pitch: p,
                        start_ms: (i as f64 * hop_ms) as i64,
                        duration_ms: (len as f64 * hop_ms) as i64,
                    });
                }
                i = j.max(i + 1);
            }
        }
    }
    notes
}

fn main() {
    let path = env::args().nth(1).expect("usage: pitch-proto <wav>");
    let (samples, sr) = read_wav_pcm16(&path);
    let notes = track(&samples, sr);
    println!("{path}: {} notes", notes.len());
    for n in &notes {
        println!("  pitch={} start={}ms dur={}ms", n.pitch, n.start_ms, n.duration_ms);
    }
}
