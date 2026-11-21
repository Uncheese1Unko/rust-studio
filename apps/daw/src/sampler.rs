//! サンプラー楽器（Issue #19）: 取り込んだ1つの音を、コンピュータ
//! キーで音程を変えて演奏する。ピッチ処理は #18 の pitch_shift
//! （ADR-0009、フォルマント保持）をそのまま使う。
//!
//! このモジュールは「ノートオン／オフ → ボイス → ミックス」という
//! 演奏の状態機械を持つ。実デバイスへの出力はアプリ側の責務で、
//! ここではボイスの生成・同時発音・終了をテストで固定する。

use crate::audio::{pitch_shift, ClipAudio};

/// コンピュータキーの配置（ピアノ式）: a を基準音（ルート）とし、
/// 半音ずつ上がる。返り値はルートからの半音数。
pub fn key_to_semitone(key: char) -> Option<i32> {
    match key {
        'a' => Some(0),
        'w' => Some(1),
        's' => Some(2),
        'e' => Some(3),
        'd' => Some(4),
        'f' => Some(5),
        't' => Some(6),
        'g' => Some(7),
        'y' => Some(8),
        'h' => Some(9),
        'u' => Some(10),
        'j' => Some(11),
        'k' => Some(12),
        _ => None,
    }
}

/// 半音数→音名（Issue #26）。鍵盤の黒鍵位置（1,3,6,8,10）が
/// C 始まりのピアノ配置なので、ルート（0 半音）を C として音名を
/// 付ける。オクターブ番号は付けない（素材の実音高は素材依存のため）。
pub fn note_name(semitones: i32) -> &'static str {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    NAMES[semitones.rem_euclid(12) as usize]
}

/// 大きなピッチ表示に出す音（Issue #26）: 最後に鳴らした音が
/// 発音中ならそれを、終わっていれば発音中の最高音を、発音中の
/// 音が無ければ None を返す（和音で最後の音だけ離したときの
/// フォールバック）。
pub fn display_note(last: Option<i32>, active: &[i32]) -> Option<i32> {
    if let Some(s) = last {
        if active.contains(&s) {
            return Some(s);
        }
    }
    active.iter().max().copied()
}

/// 発音中の1音。ピッチ変更済みの音声を持ち、再生位置が進む。
#[derive(Debug, Clone)]
pub struct Voice {
    /// ルートからの半音数。
    pub semitones: i32,
    pub audio: ClipAudio,
    /// 再生位置（フレーム）。
    pub position: usize,
    /// ノートオフ済みか（オフで即停止する。エンベロープは対象外）。
    pub released: bool,
}

impl Voice {
    pub fn finished(&self) -> bool {
        self.released || self.position * (self.audio.channels as usize) >= self.audio.samples.len()
    }
}

/// サンプラー本体: 1つの素材音と、発音中のボイス群。
pub struct Sampler {
    pub source_name: String,
    source: ClipAudio,
    voices: Vec<Voice>,
}

impl Sampler {
    pub fn new(source_name: String, source: ClipAudio) -> Self {
        Sampler {
            source_name,
            source,
            voices: Vec::new(),
        }
    }

    /// ノートオン。素材をピッチ変更したボイスを追加する。
    /// 同じ音程が既に発音中なら鳴らし直す（ボイスを差し替える）。
    pub fn note_on(&mut self, semitones: i32) {
        self.voices.retain(|v| v.semitones != semitones);
        let audio = pitch_shift(&self.source, semitones as f32);
        self.voices.push(Voice {
            semitones,
            audio,
            position: 0,
            released: false,
        });
    }

    /// ノートオフ。該当ボイスを停止状態にする。
    pub fn note_off(&mut self, semitones: i32) {
        for voice in self.voices.iter_mut() {
            if voice.semitones == semitones {
                voice.released = true;
            }
        }
    }

    /// 発音中の音程一覧（観測用）。
    pub fn active_notes(&self) -> Vec<i32> {
        let mut notes: Vec<i32> = self
            .voices
            .iter()
            .filter(|v| !v.finished())
            .map(|v| v.semitones)
            .collect();
        notes.sort();
        notes
    }

    /// 時間を進め、終わったボイスを取り除く。
    pub fn advance(&mut self, dt_ms: i64) {
        if dt_ms > 0 {
            for voice in self.voices.iter_mut() {
                let step = (dt_ms as u64 * voice.audio.sample_rate as u64 / 1000) as usize;
                voice.position += step;
            }
        }
        self.voices.retain(|v| !v.finished());
    }

    /// 指定フレーム数をミックスして返す（同時発音の確認用）。
    /// 返すのはモノラル（素材がステレオなら左チャンネル）。
    pub fn render(&mut self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames];
        for voice in self.voices.iter_mut() {
            if voice.finished() {
                continue;
            }
            let ch = voice.audio.channels.max(1) as usize;
            for (i, slot) in out.iter_mut().enumerate() {
                let idx = (voice.position + i) * ch;
                if let Some(&s) = voice.audio.samples.get(idx) {
                    *slot += s;
                }
            }
            voice.position += frames;
        }
        self.voices.retain(|v| !v.finished());
        for slot in out.iter_mut() {
            *slot = slot.clamp(-1.0, 1.0);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn sampler_220() -> Sampler {
        Sampler::new("tone.wav".into(), sine(220.0, 1000, 0.4))
    }

    #[test]
    fn note_names_follow_the_keyboard_layout() {
        // Issue #26: 現在のピッチを音名でも出すための変換。鍵盤の
        // 黒鍵位置（1,3,6,8,10）が C 始まりの配置なので、ルートを
        // C として音名を付ける。
        assert_eq!(note_name(0), "C");
        assert_eq!(note_name(1), "C#");
        assert_eq!(note_name(4), "E");
        assert_eq!(note_name(7), "G");
        assert_eq!(note_name(11), "B");
        assert_eq!(note_name(12), "C");
        assert_eq!(note_name(13), "C#");
    }

    #[test]
    fn display_note_prefers_last_played_then_highest_active() {
        // Issue #26 レビュー指摘: 最後に鳴らした音が終わっても、
        // 他の音が発音中なら大きなピッチ表示は残る音を出す。
        assert_eq!(display_note(Some(5), &[5, 7]), Some(5));
        assert_eq!(display_note(Some(5), &[0, 7]), Some(7));
        assert_eq!(display_note(None, &[3]), Some(3));
        assert_eq!(display_note(Some(5), &[]), None);
        assert_eq!(display_note(None, &[]), None);
    }

    #[test]
    fn keyboard_layout_is_chromatic_from_a() {
        assert_eq!(key_to_semitone('a'), Some(0));
        assert_eq!(key_to_semitone('w'), Some(1));
        assert_eq!(key_to_semitone('d'), Some(4));
        assert_eq!(key_to_semitone('g'), Some(7));
        assert_eq!(key_to_semitone('k'), Some(12));
        assert_eq!(key_to_semitone('z'), None);
        assert_eq!(key_to_semitone('1'), None);
    }

    #[test]
    fn note_on_plays_the_source_at_the_requested_pitch() {
        let mut sampler = sampler_220();
        sampler.note_on(12);
        assert_eq!(sampler.active_notes(), vec![12]);
        let out = sampler.render(22050);
        let mid = &out[8000..16000];
        assert!(
            segmented_goertzel(mid, RATE, 440.0) > 0.05,
            "+12 should sound at 440"
        );
        assert!(segmented_goertzel(mid, RATE, 220.0) < 0.01);
    }

    #[test]
    fn chord_sounds_all_notes_together() {
        // 同時発音: ルート（220）+ 4 半音（約 277）+ 7 半音（約 330）。
        let mut sampler = sampler_220();
        sampler.note_on(0);
        sampler.note_on(4);
        sampler.note_on(7);
        assert_eq!(sampler.active_notes(), vec![0, 4, 7]);
        let out = sampler.render(22050);
        let mid = &out[8000..16000];
        for freq in [220.0, 277.18, 329.63] {
            assert!(
                segmented_goertzel(mid, RATE, freq) > 0.02,
                "{freq} should sound in the chord"
            );
        }
    }

    #[test]
    fn note_off_and_end_of_sample_retire_voices() {
        let mut sampler = sampler_220();
        sampler.note_on(0);
        sampler.note_on(7);
        sampler.note_off(0);
        assert_eq!(sampler.active_notes(), vec![7]);
        // 素材は 1 秒。1.2 秒進めれば残りも終わる。
        sampler.advance(1200);
        assert!(sampler.active_notes().is_empty());
    }

    #[test]
    fn retrigger_replaces_the_same_note() {
        let mut sampler = sampler_220();
        sampler.note_on(0);
        sampler.advance(500);
        sampler.note_on(0);
        assert_eq!(sampler.active_notes(), vec![0]);
        // 鳴らし直しでも同じ音程は二重にならない（1ボイスだけ）
    }
}
