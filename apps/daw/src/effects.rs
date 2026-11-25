//! トラックエフェクトの DSP（Issue #92、EPIC #91、ADR-0023）。
//!
//! コア（studio-core）はエフェクトのパラメータだけを持ち、信号
//! 処理はアプリ側のここに置く（アーキテクチャルール: 重い処理は
//! コアに入れない）。依存クレートは足さず、biquad 等を自前で
//! 実装する。
//!
//! 処理対象はステレオインターリーブのバッファ。チェーンは
//! スロット順に適用され、バイパスされたスロットは読み飛ばす。
//! DSP の状態（フィルタの遅延素子など）は呼び出しごとに生成
//! するため、同じ入力には常に同じ出力が返る（render_mix の
//! 純粋性を保つ）。
//!
//! EQ は Issue #92、ディレイとリバーブ（Schroeder/Freeverb 系）
//! は Issue #93 で実装した。

use studio_core::{EffectSlot, TrackEffect};

/// EQ の帯域定義（Issue #92）。3バンドの周波数と形状は固定。
pub const EQ_LOW_FREQ_HZ: f32 = 200.0;
pub const EQ_MID_FREQ_HZ: f32 = 1_000.0;
pub const EQ_MID_Q: f32 = 1.0;
pub const EQ_HIGH_FREQ_HZ: f32 = 4_000.0;

/// RBJ Audio-EQ-Cookbook の biquad（正規化済み係数、直接形 I）。
/// 状態はチャンネルごとに1つ持つ。
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn new(b0: f32, b1: f32, b2: f32, a1: f32, a2: f32) -> Self {
        Biquad {
            b0,
            b1,
            b2,
            a1,
            a2,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// 係数の導出について: RBJ Audio-EQ-Cookbook のアナログ原型
/// （シェルフ／ピーキング）を、k = tan(w0/2) の周波数プリワープ
/// 付き双一次変換でデジタル化したもの。導出は DC・中心周波数・
/// Nyquist でのゲインで検算してある（DC でローシェルフは A^2、
/// ハイシェルフは 1、ピーキングは中心で A^2、など）。
/// 棚の Q は S = 1 相当の 1/sqrt(2) に固定する。
const SHELF_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn prewarp(sample_rate: u32, freq: f32) -> f32 {
    (std::f32::consts::PI * freq / sample_rate as f32).tan()
}

/// ローシェルフ（`freq` より下を `gain_db` だけ持ち上げ／下げる）。
/// 原型: H(s) = A (s^2 + (sqrt(A)/Q) s + A) / (A s^2 + (sqrt(A)/Q) s + 1)
fn low_shelf(sample_rate: u32, freq: f32, gain_db: f32) -> Biquad {
    let a = 10f32.powf(gain_db / 40.0);
    let k = prewarp(sample_rate, freq);
    let k2 = k * k;
    let d = a.sqrt() * k / SHELF_Q;
    let b0 = a * (1.0 + d + a * k2);
    let b1 = a * (-2.0 + 2.0 * a * k2);
    let b2 = a * (1.0 - d + a * k2);
    let a0 = a + d + k2;
    let a1 = -2.0 * a + 2.0 * k2;
    let a2 = a - d + k2;
    Biquad::new(b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
}

/// ハイシェルフ（`freq` より上を `gain_db` だけ持ち上げ／下げる）。
/// 原型: H(s) = A (A s^2 + (sqrt(A)/Q) s + 1) / (s^2 + (sqrt(A)/Q) s + A)
fn high_shelf(sample_rate: u32, freq: f32, gain_db: f32) -> Biquad {
    let a = 10f32.powf(gain_db / 40.0);
    let k = prewarp(sample_rate, freq);
    let k2 = k * k;
    let d = a.sqrt() * k / SHELF_Q;
    let b0 = a * (a + d + k2);
    let b1 = a * (-2.0 * a + 2.0 * k2);
    let b2 = a * (a - d + k2);
    let a0 = 1.0 + d + a * k2;
    let a1 = -2.0 + 2.0 * a * k2;
    let a2 = 1.0 - d + a * k2;
    Biquad::new(b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
}

/// ピーキング（中心 `freq` を Q の幅で `gain_db` だけ増減する）。
/// 中心周波数でのゲインはちょうど `gain_db` になる。
/// 原型: H(s) = (s^2 + (A/Q) s + 1) / (s^2 + (1/(A Q)) s + 1)
fn peaking(sample_rate: u32, freq: f32, q: f32, gain_db: f32) -> Biquad {
    let a = 10f32.powf(gain_db / 40.0);
    let k = prewarp(sample_rate, freq);
    let k2 = k * k;
    let e = a * k / q;
    let f = k / (a * q);
    let b0 = 1.0 + e + k2;
    let b1 = -2.0 + 2.0 * k2;
    let b2 = 1.0 - e + k2;
    let a0 = 1.0 + f + k2;
    let a1 = -2.0 + 2.0 * k2;
    let a2 = 1.0 - f + k2;
    Biquad::new(b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
}

/// 3バンドEQ の処理状態（チャンネルごとに3本の biquad）。
struct Eq3State {
    /// [チャンネル][low, mid, high]
    bands: [[Biquad; 3]; 2],
}

impl Eq3State {
    fn new(sample_rate: u32, low_db: f32, mid_db: f32, high_db: f32) -> Self {
        let make = || {
            [
                low_shelf(sample_rate, EQ_LOW_FREQ_HZ, low_db),
                peaking(sample_rate, EQ_MID_FREQ_HZ, EQ_MID_Q, mid_db),
                high_shelf(sample_rate, EQ_HIGH_FREQ_HZ, high_db),
            ]
        };
        Eq3State {
            bands: [make(), make()],
        }
    }

    fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.chunks_mut(2) {
            for (ch, slot) in frame.iter_mut().enumerate() {
                let mut v = *slot;
                for band in &mut self.bands[ch] {
                    v = band.process(v);
                }
                *slot = v;
            }
        }
    }
}

/// フィードバックディレイの処理状態（Issue #93）。
/// チャンネルごとの循環バッファを持ち、出力 = ドライ + mix ×
/// ウェット、遅延入力 = 入力 + feedback × ウェット。
struct DelayState {
    buffers: [Vec<f32>; 2],
    pos: usize,
    feedback: f32,
    mix: f32,
}

impl DelayState {
    fn new(sample_rate: u32, time_ms: i64, feedback: f32, mix: f32) -> Self {
        let len = ((time_ms as u64 * sample_rate as u64) / 1000).max(1) as usize;
        DelayState {
            buffers: [vec![0.0; len], vec![0.0; len]],
            pos: 0,
            feedback,
            mix,
        }
    }

    fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.chunks_mut(2) {
            for (ch, slot) in frame.iter_mut().enumerate() {
                let wet = self.buffers[ch][self.pos];
                let dry = *slot;
                self.buffers[ch][self.pos] = dry + wet * self.feedback;
                *slot = dry + wet * self.mix;
            }
            self.pos = (self.pos + 1) % self.buffers[0].len();
        }
    }
}

/// コムフィルタ（Freeverb の構成要素）。ループ内に一次ローパス
/// （ダンピング）を持つ。
struct Comb {
    buffer: Vec<f32>,
    pos: usize,
    feedback: f32,
    damp1: f32,
    damp2: f32,
    store: f32,
}

impl Comb {
    fn new(len: usize, feedback: f32, damp: f32) -> Self {
        Comb {
            buffer: vec![0.0; len.max(1)],
            pos: 0,
            feedback,
            damp1: 1.0 - damp,
            damp2: damp,
            store: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.buffer[self.pos];
        self.store = output * self.damp2 + self.store * self.damp1;
        self.buffer[self.pos] = input + self.store * self.feedback;
        self.pos = (self.pos + 1) % self.buffer.len();
        output
    }
}

/// オールパスフィルタ（Freeverb の構成要素、ゲイン 0.5）。
struct Allpass {
    buffer: Vec<f32>,
    pos: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Allpass {
            buffer: vec![0.0; len.max(1)],
            pos: 0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buffered = self.buffer[self.pos];
        let output = buffered - input;
        self.buffer[self.pos] = input + buffered * 0.5;
        self.pos = (self.pos + 1) % self.buffer.len();
        output
    }
}

/// リバーブの処理状態（Issue #93、Schroeder/Freeverb 系）。
/// チャンネルごとにコム8本の並列 → オールパス4本の直列。
/// 遅延量は Freeverb の 44.1kHz 基準値をサンプルレート比で
/// スケーリングし、右チャンネルにはステレオ広がりのずらしを
/// 加える。コムへの入力ゲインは Freeverb と同じ 0.015 に
/// 基づき、8本の合算でも破綻しないよう調整した固定値。
struct ReverbState {
    combs: [[Comb; 8]; 2],
    allpasses: [[Allpass; 4]; 2],
    mix: f32,
}

const COMB_TUNINGS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNINGS: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;
const REVERB_INPUT_GAIN: f32 = 0.2;
const REVERB_DAMP: f32 = 0.2;

impl ReverbState {
    fn new(sample_rate: u32, decay: f32, mix: f32) -> Self {
        // decay 0.0..=1.0 をコムのフィードバック 0.7..=0.98 へ写像
        let feedback = 0.7 + 0.28 * decay;
        let scale = sample_rate as f32 / 44_100.0;
        let scaled = |base: usize, spread: usize| -> usize {
            (((base + spread) as f32) * scale).round().max(1.0) as usize
        };
        let make_combs = |spread: usize| {
            COMB_TUNINGS.map(|t| Comb::new(scaled(t, spread), feedback, REVERB_DAMP))
        };
        let make_allpasses =
            |spread: usize| ALLPASS_TUNINGS.map(|t| Allpass::new(scaled(t, spread)));
        ReverbState {
            combs: [make_combs(0), make_combs(STEREO_SPREAD)],
            allpasses: [make_allpasses(0), make_allpasses(STEREO_SPREAD)],
            mix,
        }
    }

    fn process(&mut self, samples: &mut [f32]) {
        for frame in samples.chunks_mut(2) {
            for (ch, slot) in frame.iter_mut().enumerate() {
                let dry = *slot;
                let input = dry * REVERB_INPUT_GAIN;
                let mut wet: f32 = self.combs[ch]
                    .iter_mut()
                    .map(|comb| comb.process(input))
                    .sum();
                for allpass in &mut self.allpasses[ch] {
                    wet = allpass.process(wet);
                }
                *slot = dry + wet * self.mix;
            }
        }
    }
}

/// エフェクトチェーンをステレオインターリーブのバッファへ
/// スロット順に適用する。バイパスされたスロットは読み飛ばす。
/// チェーンが空なら何もしない（従来と同一の音）。
pub fn process_chain(effects: &[EffectSlot], samples: &mut [f32], sample_rate: u32) {
    for slot in effects {
        if slot.bypassed {
            continue;
        }
        match slot.effect {
            TrackEffect::Eq3 {
                low_gain_db,
                mid_gain_db,
                high_gain_db,
            } => {
                let mut state = Eq3State::new(sample_rate, low_gain_db, mid_gain_db, high_gain_db);
                state.process(samples);
            }
            TrackEffect::Delay {
                time_ms,
                feedback,
                mix,
            } => {
                let mut state = DelayState::new(sample_rate, time_ms, feedback, mix);
                state.process(samples);
            }
            TrackEffect::Reverb { decay, mix } => {
                let mut state = ReverbState::new(sample_rate, decay, mix);
                state.process(samples);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44100;

    fn sine(freq: f32, seconds: f32, amp: f32) -> Vec<f32> {
        let frames = (RATE as f32 * seconds) as usize;
        let mut out = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let v = amp * (2.0 * std::f32::consts::PI * freq * i as f32 / RATE as f32).sin();
            out.push(v);
            out.push(v);
        }
        out
    }

    fn rms_left(samples: &[f32]) -> f32 {
        let left: Vec<f32> = samples.iter().step_by(2).cloned().collect();
        (left.iter().map(|s| s * s).sum::<f32>() / left.len() as f32).sqrt()
    }

    fn gain_db(freq: f32, amp: f32, effect: TrackEffect) -> f32 {
        let input = sine(freq, 1.0, amp);
        let mut output = input.clone();
        process_chain(
            &[EffectSlot {
                effect,
                bypassed: false,
            }],
            &mut output,
            RATE,
        );
        // 過渡を避けて後半だけで測る
        let half = input.len() / 2;
        20.0 * (rms_left(&output[half..]) / rms_left(&input[half..])).log10()
    }

    #[test]
    fn eq_bands_measure_configured_gain() {
        // DSP 単体の測定（render_mix を介さない）。
        let low = gain_db(
            50.0,
            0.1,
            TrackEffect::Eq3 {
                low_gain_db: 12.0,
                mid_gain_db: 0.0,
                high_gain_db: 0.0,
            },
        );
        assert!((low - 12.0).abs() <= 1.5, "low band: {low:.2} dB");
        let mid = gain_db(
            1_000.0,
            0.4,
            TrackEffect::Eq3 {
                low_gain_db: 0.0,
                mid_gain_db: -12.0,
                high_gain_db: 0.0,
            },
        );
        assert!((mid + 12.0).abs() <= 1.5, "mid band: {mid:.2} dB");
        let high = gain_db(
            10_000.0,
            0.1,
            TrackEffect::Eq3 {
                low_gain_db: 0.0,
                mid_gain_db: 0.0,
                high_gain_db: 12.0,
            },
        );
        assert!((high - 12.0).abs() <= 1.5, "high band: {high:.2} dB");
    }

    fn impulse(seconds: f32) -> Vec<f32> {
        let frames = (RATE as f32 * seconds) as usize;
        let mut out = vec![0.0f32; frames * 2];
        out[0] = 1.0;
        out[1] = 1.0;
        out
    }

    fn at(samples: &[f32], ms: f32) -> f32 {
        let frame = (RATE as f32 * ms / 1000.0) as usize;
        samples[frame * 2]
    }

    fn peak_in(samples: &[f32], from_ms: f32, to_ms: f32) -> f32 {
        let a = (RATE as f32 * from_ms / 1000.0) as usize;
        let b = (RATE as f32 * to_ms / 1000.0) as usize;
        samples[a * 2..b * 2]
            .iter()
            .step_by(2)
            .map(|v| v.abs())
            .fold(0.0f32, f32::max)
    }

    #[test]
    fn delay_produces_echoes_at_configured_time() {
        // Issue #93: インパルス → 200ms ごとに mix、mix×feedback と
        // 減衰するエコーが立つ。ドライは原音のまま残る。
        let mut out = impulse(1.0);
        process_chain(
            &[EffectSlot {
                effect: TrackEffect::Delay {
                    time_ms: 200,
                    feedback: 0.5,
                    mix: 0.5,
                },
                bypassed: false,
            }],
            &mut out,
            RATE,
        );
        assert_eq!(at(&out, 0.0), 1.0, "ドライのインパルスはそのまま");
        assert_eq!(at(&out, 100.0), 0.0, "エコー前は無音");
        assert!(
            (at(&out, 200.0) - 0.5).abs() < 0.02,
            "1回目: {}",
            at(&out, 200.0)
        );
        assert!(
            (at(&out, 400.0) - 0.25).abs() < 0.02,
            "2回目: {}",
            at(&out, 400.0)
        );
        assert!(
            (at(&out, 600.0) - 0.125).abs() < 0.02,
            "3回目: {}",
            at(&out, 600.0)
        );
    }

    #[test]
    fn delay_zero_mix_and_bypass_leave_input_untouched() {
        let input = impulse(0.5);
        let mut out = input.clone();
        process_chain(
            &[EffectSlot {
                effect: TrackEffect::Delay {
                    time_ms: 200,
                    feedback: 0.5,
                    mix: 0.0,
                },
                bypassed: false,
            }],
            &mut out,
            RATE,
        );
        assert_eq!(input, out, "mix 0 ではエコーが混ざらない");
        process_chain(
            &[EffectSlot {
                effect: TrackEffect::Delay {
                    time_ms: 200,
                    feedback: 0.5,
                    mix: 0.5,
                },
                bypassed: true,
            }],
            &mut out,
            RATE,
        );
        assert_eq!(input, out, "バイパスでは素通り");
    }

    #[test]
    fn reverb_extends_impulse_tail() {
        // Issue #93: decay 0.8 では 1 秒付近でも残響が -60dB を
        // 上回り、decay が小さい場合より尾が長い。決定的である。
        let run = |decay: f32| {
            let mut out = impulse(2.0);
            process_chain(
                &[EffectSlot {
                    effect: TrackEffect::Reverb { decay, mix: 1.0 },
                    bypassed: false,
                }],
                &mut out,
                RATE,
            );
            out
        };
        let wet = run(0.8);
        let tail = peak_in(&wet, 900.0, 1_000.0);
        assert!(tail > 0.001, "1秒付近の残響が -60dB を上回る: peak {tail}");
        let short = run(0.2);
        let short_tail = peak_in(&short, 900.0, 1_000.0);
        assert!(
            short_tail < tail,
            "decay が小さい方が尾が短い: {short_tail} vs {tail}"
        );
        // ドライ（インパルスそのもの）は 500ms 以降無音
        let dry = impulse(2.0);
        assert_eq!(peak_in(&dry, 500.0, 1_000.0), 0.0);
        // 決定的: 同じ入力には同じ出力
        assert_eq!(wet, run(0.8));
    }

    #[test]
    fn chain_composes_eq_into_delay_echoes() {
        // Issue #93（順序の構成測定）: [EQ → Delay] ではエコー位置に
        // EQ のインパルス応答が mix 倍で現れる。
        let eq = TrackEffect::Eq3 {
            low_gain_db: 12.0,
            mid_gain_db: 0.0,
            high_gain_db: 0.0,
        };
        let delay = TrackEffect::Delay {
            time_ms: 200,
            feedback: 0.0,
            mix: 0.5,
        };
        let mut eq_only = impulse(0.5);
        process_chain(
            &[EffectSlot {
                effect: eq,
                bypassed: false,
            }],
            &mut eq_only,
            RATE,
        );
        let eq_peak = peak_in(&eq_only, 0.0, 20.0);
        let mut chained = impulse(0.5);
        process_chain(
            &[
                EffectSlot {
                    effect: eq,
                    bypassed: false,
                },
                EffectSlot {
                    effect: delay,
                    bypassed: false,
                },
            ],
            &mut chained,
            RATE,
        );
        let echo_peak = peak_in(&chained, 195.0, 215.0);
        assert!(
            (echo_peak - 0.5 * eq_peak).abs() <= 0.25 * eq_peak,
            "エコーは EQ 応答の mix 倍: echo {echo_peak}, eq {eq_peak}"
        );
        // チェーン出力は Delay 単体（EQ なし）とも異なる
        let mut delay_only = impulse(0.5);
        process_chain(
            &[EffectSlot {
                effect: delay,
                bypassed: false,
            }],
            &mut delay_only,
            RATE,
        );
        assert_ne!(chained, delay_only);
    }

    #[test]
    fn bypassed_and_empty_chain_leave_samples_untouched() {
        let input = sine(440.0, 0.5, 0.3);
        let mut out = input.clone();
        process_chain(&[], &mut out, RATE);
        assert_eq!(input, out);
        process_chain(
            &[EffectSlot {
                effect: TrackEffect::Eq3 {
                    low_gain_db: 12.0,
                    mid_gain_db: 12.0,
                    high_gain_db: 12.0,
                },
                bypassed: true,
            }],
            &mut out,
            RATE,
        );
        assert_eq!(input, out);
    }
}
