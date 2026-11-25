//! エフェクトパネル（Issue #94）のレイアウトと当たり判定。
//!
//! GUI（nannou）に依存しない純粋関数として置き、単体テストで
//! 固める（tones.rs と同じ流儀）。パネルは音色パネルの下に置き、
//! 選択中トラックのエフェクトチェーンをチップ列で出し、末尾に
//! 追加（+REV/+DLY/+EQ）と選択スロットへの操作（DEL/↑/↓/BYP）の
//! ボタンを並べる。選択中スロットのパラメータは下段にスライダー
//! 行で出す。コマンドの適用とジェスチャ管理は main.rs 側の責務で、
//! ここは座標⇔操作とパラメータ値の変換だけを持つ。

use studio_core::{EffectSlot, Tone, TrackEffect};

use crate::mixer;
use crate::tones;

/// パネル上端からチップ行までの余白（ラベル分、px）。
pub const ROWS_TOP_OFFSET_PX: f32 = 16.0;
/// チップ行の高さ（px）。
pub const ROW_H_PX: f32 = 26.0;
/// チップの高さ（px）。
pub const CHIP_H_PX: f32 = 20.0;
/// チップ列の開始 x（px）。音色パネルに揃える。
pub const CHIP_X_PX: f32 = 72.0;
/// アイテム間の余白（px）。
pub const CHIP_GAP_PX: f32 = 6.0;
/// スロットチップの幅（px）。
pub const SLOT_W_PX: f32 = 92.0;
/// パラメータ行の高さ（px）。
pub const PARAM_ROW_H_PX: f32 = 22.0;
/// スライダーの開始 x（px）。左にパラメータ名を置く。
pub const SLIDER_X_PX: f32 = 190.0;
/// スライダーの幅（px）。
pub const SLIDER_W_PX: f32 = 320.0;
/// スライダーの高さ（px）。
pub const SLIDER_H_PX: f32 = 14.0;

/// パネルの上端 y。音色パネルの下に 12px 空けて置く。
pub fn panel_top(track_count: usize, tones: &[Tone], limit_x: f32) -> f32 {
    tones::panel_top(track_count) + tones::panel_height(tones, limit_x) + 12.0
}

/// パネルの高さ（ラベル＋チップ行＋選択中スロットのパラメータ行）。
pub fn panel_height(selected: Option<&TrackEffect>) -> f32 {
    let params = selected.map(param_count).unwrap_or(0);
    ROWS_TOP_OFFSET_PX + ROW_H_PX + params as f32 * PARAM_ROW_H_PX + 6.0
}

/// スロットの短い名前（チップ表示用）。
pub fn slot_label(effect: &TrackEffect) -> &'static str {
    match effect {
        TrackEffect::Reverb { .. } => "REVERB",
        TrackEffect::Delay { .. } => "DELAY",
        TrackEffect::Eq3 { .. } => "EQ",
    }
}

/// パラメータの数。
pub fn param_count(effect: &TrackEffect) -> usize {
    match effect {
        TrackEffect::Reverb { .. } => 2,
        TrackEffect::Delay { .. } | TrackEffect::Eq3 { .. } => 3,
    }
}

/// パラメータの名前（スライダー行の見出し）。
pub fn param_label(effect: &TrackEffect, index: usize) -> &'static str {
    match (effect, index) {
        (TrackEffect::Reverb { .. }, 0) => "DECAY",
        (TrackEffect::Reverb { .. }, _) => "MIX",
        (TrackEffect::Delay { .. }, 0) => "TIME",
        (TrackEffect::Delay { .. }, 1) => "FDBK",
        (TrackEffect::Delay { .. }, _) => "MIX",
        (TrackEffect::Eq3 { .. }, 0) => "LOW",
        (TrackEffect::Eq3 { .. }, 1) => "MID",
        (TrackEffect::Eq3 { .. }, _) => "HIGH",
    }
}

/// パラメータの範囲（コマンド層の検証範囲と一致させる）。
pub fn param_range(effect: &TrackEffect, index: usize) -> (f32, f32) {
    match (effect, index) {
        (TrackEffect::Delay { .. }, 0) => (1.0, 2000.0),
        (TrackEffect::Delay { .. }, 1) => (0.0, 0.9),
        (TrackEffect::Eq3 { .. }, _) => (-24.0, 24.0),
        _ => (0.0, 1.0),
    }
}

/// パラメータの現在値。
pub fn param_value(effect: &TrackEffect, index: usize) -> f32 {
    match (effect, index) {
        (TrackEffect::Reverb { decay, .. }, 0) => *decay,
        (TrackEffect::Reverb { mix, .. }, _) => *mix,
        (TrackEffect::Delay { time_ms, .. }, 0) => *time_ms as f32,
        (TrackEffect::Delay { feedback, .. }, 1) => *feedback,
        (TrackEffect::Delay { mix, .. }, _) => *mix,
        (TrackEffect::Eq3 { low_gain_db, .. }, 0) => *low_gain_db,
        (TrackEffect::Eq3 { mid_gain_db, .. }, 1) => *mid_gain_db,
        (TrackEffect::Eq3 { high_gain_db, .. }, _) => *high_gain_db,
    }
}

/// パラメータを差し替えたエフェクトを返す。値は範囲へ丸める
/// （TIME はミリ秒の整数へ）。
pub fn with_param(effect: &TrackEffect, index: usize, value: f32) -> TrackEffect {
    let (min, max) = param_range(effect, index);
    let v = value.clamp(min, max);
    match (effect, index) {
        (TrackEffect::Reverb { mix, .. }, 0) => TrackEffect::Reverb {
            decay: v,
            mix: *mix,
        },
        (TrackEffect::Reverb { decay, .. }, _) => TrackEffect::Reverb {
            decay: *decay,
            mix: v,
        },
        (TrackEffect::Delay { feedback, mix, .. }, 0) => TrackEffect::Delay {
            time_ms: v.round() as i64,
            feedback: *feedback,
            mix: *mix,
        },
        (TrackEffect::Delay { time_ms, mix, .. }, 1) => TrackEffect::Delay {
            time_ms: *time_ms,
            feedback: v,
            mix: *mix,
        },
        (
            TrackEffect::Delay {
                time_ms, feedback, ..
            },
            _,
        ) => TrackEffect::Delay {
            time_ms: *time_ms,
            feedback: *feedback,
            mix: v,
        },
        (
            TrackEffect::Eq3 {
                mid_gain_db,
                high_gain_db,
                ..
            },
            0,
        ) => TrackEffect::Eq3 {
            low_gain_db: v,
            mid_gain_db: *mid_gain_db,
            high_gain_db: *high_gain_db,
        },
        (
            TrackEffect::Eq3 {
                low_gain_db,
                high_gain_db,
                ..
            },
            1,
        ) => TrackEffect::Eq3 {
            low_gain_db: *low_gain_db,
            mid_gain_db: v,
            high_gain_db: *high_gain_db,
        },
        (
            TrackEffect::Eq3 {
                low_gain_db,
                mid_gain_db,
                ..
            },
            _,
        ) => TrackEffect::Eq3 {
            low_gain_db: *low_gain_db,
            mid_gain_db: *mid_gain_db,
            high_gain_db: v,
        },
    }
}

/// パラメータ値の表示文字列。
pub fn format_value(effect: &TrackEffect, index: usize) -> String {
    let v = param_value(effect, index);
    match (effect, index) {
        (TrackEffect::Delay { .. }, 0) => format!("{}ms", v as i64),
        (TrackEffect::Eq3 { .. }, _) => format!("{v:+.1}dB"),
        _ => format!("{v:.2}"),
    }
}

/// チップ行のボタン。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxButton {
    AddReverb,
    AddDelay,
    AddEq,
    Delete,
    Up,
    Down,
    Bypass,
}

impl FxButton {
    pub fn label(self) -> &'static str {
        match self {
            FxButton::AddReverb => "+REV",
            FxButton::AddDelay => "+DLY",
            FxButton::AddEq => "+EQ",
            FxButton::Delete => "DEL",
            FxButton::Up => "↑",
            FxButton::Down => "↓",
            FxButton::Bypass => "BYP",
        }
    }

    fn width(self) -> f32 {
        match self {
            FxButton::AddReverb | FxButton::AddDelay => 52.0,
            FxButton::AddEq | FxButton::Delete | FxButton::Bypass => 46.0,
            FxButton::Up | FxButton::Down => 26.0,
        }
    }

    const ALL: [FxButton; 7] = [
        FxButton::AddReverb,
        FxButton::AddDelay,
        FxButton::AddEq,
        FxButton::Delete,
        FxButton::Up,
        FxButton::Down,
        FxButton::Bypass,
    ];
}

/// チップ行のフロー配置アイテム。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlowItem {
    Slot(usize),
    Button(FxButton),
}

fn flow_layout(effects_len: usize, limit_x: f32) -> Vec<(FlowItem, f32, usize)> {
    let mut items: Vec<FlowItem> = (0..effects_len).map(FlowItem::Slot).collect();
    items.extend(FxButton::ALL.iter().map(|b| FlowItem::Button(*b)));
    let width = |item: FlowItem| match item {
        FlowItem::Slot(_) => SLOT_W_PX,
        FlowItem::Button(b) => b.width(),
    };
    let mut placed = Vec::new();
    let mut x = CHIP_X_PX;
    let mut row = 0usize;
    for item in items {
        let w = width(item);
        if x > CHIP_X_PX && x + w > limit_x {
            row += 1;
            x = CHIP_X_PX;
        }
        placed.push((item, x, row));
        x += w + CHIP_GAP_PX;
    }
    placed
}

fn flow_rect(panel_top_y: f32, effects_len: usize, target: FlowItem, limit_x: f32) -> mixer::Rect {
    for (item, x, row) in flow_layout(effects_len, limit_x) {
        if item == target {
            let w = match item {
                FlowItem::Slot(_) => SLOT_W_PX,
                FlowItem::Button(b) => b.width(),
            };
            return mixer::Rect {
                x,
                y: panel_top_y
                    + ROWS_TOP_OFFSET_PX
                    + row as f32 * ROW_H_PX
                    + (ROW_H_PX - CHIP_H_PX) / 2.0,
                w,
                h: CHIP_H_PX,
            };
        }
    }
    unreachable!("flow items always include the target");
}

/// スロットチップの矩形。
pub fn slot_rect(panel_top_y: f32, effects_len: usize, index: usize, limit_x: f32) -> mixer::Rect {
    flow_rect(panel_top_y, effects_len, FlowItem::Slot(index), limit_x)
}

/// ボタンの矩形。
pub fn button_rect(
    panel_top_y: f32,
    effects_len: usize,
    button: FxButton,
    limit_x: f32,
) -> mixer::Rect {
    flow_rect(panel_top_y, effects_len, FlowItem::Button(button), limit_x)
}

/// パラメータ行のスライダー矩形。チップ行が折り返した場合は
/// その分だけ下がる。
pub fn slider_rect(
    panel_top_y: f32,
    effects_len: usize,
    index: usize,
    limit_x: f32,
) -> mixer::Rect {
    let rows = flow_layout(effects_len, limit_x)
        .iter()
        .map(|(_, _, row)| *row)
        .max()
        .unwrap_or(0)
        + 1;
    mixer::Rect {
        x: SLIDER_X_PX,
        y: panel_top_y
            + ROWS_TOP_OFFSET_PX
            + rows as f32 * ROW_H_PX
            + index as f32 * PARAM_ROW_H_PX
            + (PARAM_ROW_H_PX - SLIDER_H_PX) / 2.0,
        w: SLIDER_W_PX,
        h: SLIDER_H_PX,
    }
}

/// スライダーの x から正規化値（0.0..=1.0）を得る。
pub fn value_at(rect: mixer::Rect, x: f32) -> f32 {
    ((x - rect.x) / rect.w).clamp(0.0, 1.0)
}

/// 正規化値からスライダー上の x を得る。
pub fn x_for_value(rect: mixer::Rect, normalized: f32) -> f32 {
    rect.x + normalized.clamp(0.0, 1.0) * rect.w
}

/// パラメータ値と正規化値の相互変換。
pub fn normalize(effect: &TrackEffect, index: usize, value: f32) -> f32 {
    let (min, max) = param_range(effect, index);
    (value - min) / (max - min)
}

pub fn denormalize(effect: &TrackEffect, index: usize, normalized: f32) -> f32 {
    let (min, max) = param_range(effect, index);
    min + normalized.clamp(0.0, 1.0) * (max - min)
}

/// パネルへのヒット。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxHit {
    /// スロットチップ（チェーン内の位置）。
    Slot(usize),
    /// チップ行のボタン。
    Button(FxButton),
    /// パラメータスライダー（パラメータの位置）。
    Slider(usize),
}

/// パネルへのヒットテスト。対象は選択中トラックのチェーン
/// （`effects`）と選択中スロット（`selected`）。DEL/↑/↓/BYP は
/// スロットが選択されているときだけ当たる。
pub fn hit_fx(
    track_count: usize,
    tones: &[Tone],
    effects: &[EffectSlot],
    selected: Option<usize>,
    x: f32,
    y: f32,
    limit_x: f32,
) -> Option<FxHit> {
    let top = panel_top(track_count, tones, limit_x);
    for index in 0..effects.len() {
        if slot_rect(top, effects.len(), index, limit_x).contains(x, y) {
            return Some(FxHit::Slot(index));
        }
    }
    let has_selection = selected.is_some_and(|i| i < effects.len());
    for button in FxButton::ALL {
        let needs_selection = matches!(
            button,
            FxButton::Delete | FxButton::Up | FxButton::Down | FxButton::Bypass
        );
        if needs_selection && !has_selection {
            continue;
        }
        if button_rect(top, effects.len(), button, limit_x).contains(x, y) {
            return Some(FxHit::Button(button));
        }
    }
    if let Some(sel) = selected.filter(|i| *i < effects.len()) {
        let effect = &effects[sel].effect;
        for index in 0..param_count(effect) {
            if slider_rect(top, effects.len(), index, limit_x).contains(x, y) {
                return Some(FxHit::Slider(index));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::Instrument;

    fn tone(id: u64) -> Tone {
        Tone {
            id,
            name: "Pad".into(),
            instrument: Instrument::Sampler {
                source: "a.wav".into(),
                root_note: 60,
            },
        }
    }

    fn slot(effect: TrackEffect) -> EffectSlot {
        EffectSlot {
            effect,
            bypassed: false,
        }
    }

    #[test]
    fn panel_sits_below_tones() {
        let tones = vec![tone(1)];
        assert!(
            panel_top(2, &tones, 944.0) >= tones::panel_top(2) + tones::panel_height(&tones, 944.0)
        );
        // パラメータ行の分だけ高さが伸びる
        let rev = TrackEffect::default_reverb();
        assert!(panel_height(Some(&rev)) > panel_height(None));
        assert_eq!(
            panel_height(Some(&TrackEffect::default_delay())),
            panel_height(Some(&TrackEffect::Eq3 {
                low_gain_db: 0.0,
                mid_gain_db: 0.0,
                high_gain_db: 0.0
            }))
        );
    }

    #[test]
    fn hit_fx_maps_slots_buttons_and_sliders() {
        let tones = vec![tone(1)];
        let effects = vec![
            slot(TrackEffect::default_reverb()),
            slot(TrackEffect::default_delay()),
        ];
        let top = panel_top(2, &tones, 944.0);
        let chip0 = slot_rect(top, effects.len(), 0, 944.0);
        assert_eq!(
            hit_fx(
                2,
                &tones,
                &effects,
                None,
                chip0.x + 5.0,
                chip0.y + 5.0,
                944.0
            ),
            Some(FxHit::Slot(0))
        );
        let add = button_rect(top, effects.len(), FxButton::AddEq, 944.0);
        assert_eq!(
            hit_fx(2, &tones, &effects, None, add.x + 5.0, add.y + 5.0, 944.0),
            Some(FxHit::Button(FxButton::AddEq))
        );
        // 選択が無いとき DEL には当たらない
        let del = button_rect(top, effects.len(), FxButton::Delete, 944.0);
        assert_eq!(
            hit_fx(2, &tones, &effects, None, del.x + 5.0, del.y + 5.0, 944.0),
            None
        );
        assert_eq!(
            hit_fx(
                2,
                &tones,
                &effects,
                Some(1),
                del.x + 5.0,
                del.y + 5.0,
                944.0
            ),
            Some(FxHit::Button(FxButton::Delete))
        );
        // 選択中スロット（Delay）のスライダーに当たる
        let slider = slider_rect(top, effects.len(), 0, 944.0);
        assert_eq!(
            hit_fx(
                2,
                &tones,
                &effects,
                Some(1),
                slider.x + 5.0,
                slider.y + 5.0,
                944.0
            ),
            Some(FxHit::Slider(0))
        );
        // パネル外は対象外
        assert_eq!(
            hit_fx(2, &tones, &effects, Some(1), 30.0, chip0.y + 5.0, 944.0),
            None
        );
    }

    #[test]
    fn param_roundtrip_and_clamping() {
        let delay = TrackEffect::Delay {
            time_ms: 250,
            feedback: 0.3,
            mix: 0.3,
        };
        assert_eq!(param_value(&delay, 0), 250.0);
        assert_eq!(
            with_param(&delay, 0, 480.4),
            TrackEffect::Delay {
                time_ms: 480,
                feedback: 0.3,
                mix: 0.3
            }
        );
        // 範囲外は丸める（コマンド層の検証範囲と一致）
        assert_eq!(
            with_param(&delay, 1, 2.0),
            TrackEffect::Delay {
                time_ms: 250,
                feedback: 0.9,
                mix: 0.3
            }
        );
        let eq = TrackEffect::Eq3 {
            low_gain_db: 0.0,
            mid_gain_db: 0.0,
            high_gain_db: 0.0,
        };
        assert_eq!(
            with_param(&eq, 2, -30.0),
            TrackEffect::Eq3 {
                low_gain_db: 0.0,
                mid_gain_db: 0.0,
                high_gain_db: -24.0
            }
        );
        let rev = TrackEffect::Reverb {
            decay: 0.5,
            mix: 0.3,
        };
        assert_eq!(param_count(&rev), 2);
        assert_eq!(param_label(&rev, 0), "DECAY");
        assert_eq!(param_label(&delay, 1), "FDBK");
        assert_eq!(format_value(&delay, 0), "250ms");
        assert_eq!(format_value(&eq, 0), "+0.0dB");
        assert_eq!(format_value(&rev, 1), "0.30");
    }

    #[test]
    fn slider_normalization_roundtrips() {
        let delay = TrackEffect::default_delay();
        let rect = mixer::Rect {
            x: 190.0,
            y: 0.0,
            w: 320.0,
            h: 14.0,
        };
        assert_eq!(value_at(rect, 190.0), 0.0);
        assert_eq!(value_at(rect, 510.0), 1.0);
        assert_eq!(value_at(rect, 0.0), 0.0);
        assert_eq!(x_for_value(rect, 0.5), 350.0);
        // TIME 1..=2000 の中点は約 1000ms
        let mid = denormalize(&delay, 0, 0.5);
        assert!((mid - 1000.5).abs() < 0.6, "{mid}");
        assert!((normalize(&delay, 0, mid) - 0.5).abs() < 0.001);
    }
}
