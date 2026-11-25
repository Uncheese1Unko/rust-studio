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
//! Issue #92 の範囲は EQ のみ。Reverb / Delay は素通りで、
//! DSP は Issue #93 で入れる。

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
            // Issue #93 で DSP を入れる。それまでは素通り。
            TrackEffect::Reverb { .. } | TrackEffect::Delay { .. } => {}
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
