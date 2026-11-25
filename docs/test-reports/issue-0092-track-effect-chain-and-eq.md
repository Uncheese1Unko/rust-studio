# 試験レポート: #92 トラックにエフェクトチェーンを持たせ、EQ を掛けられる（コア＋チェーン適用の土台）

対象 PBI: #92（EPIC #91「DAW トラックエフェクト」の1件目）
実施日: 2026-11-25
実装コミット: `24716d1`（本体）、`f927e0b`（レビュー観察への対応: 測定ケース追加）
ベース: `c5ddfcf`

## 実装内容（ADR-0023）

- `crates/studio-core/src/model.rs`: `TrackEffect`（Reverb { decay, mix } / Delay { time_ms, feedback, mix } / Eq3 { low/mid/high_gain_db }）と `EffectSlot { effect, bypassed }` を追加し、`Track.effects: Vec<EffectSlot>`（serde default で旧形式互換）を持たせた。既定値コンストラクタ `default_reverb/delay/eq3` も追加。
- `crates/studio-core/src/command.rs`: コマンド5種（AddTrackEffect＝戻り値はスロット位置、RemoveTrackEffect、MoveTrackEffect、SetTrackEffect＝バイパス保持、SetTrackEffectBypass）と `validate_effect`（範囲＋有限チェック、失敗時は `InvalidRange("effect parameter out of range")` で状態不変）。
- `apps/daw/src/effects.rs`（新規）: DSP。EQ は RBJ Cookbook のアナログ原型（ローシェルフ 200Hz／ピーキング 1kHz Q=1／ハイシェルフ 4kHz、棚 Q は S=1 相当）を k=tan(w0/2) の双一次変換でデジタル化した biquad。Reverb / Delay は本 PBI では素通り（DSP は #93）。
- `apps/daw/src/audio.rs`: `render_mix` をトラック単位処理へ組み替え（トラックごとにドライをバッファへ描画 → チェーン適用 → 音量・パン（ポストフェーダー）で合算 → 最後にクランプ）。チェーン空のトラックは従来と同一の結果になる。
- `crates/studio-mcp/src/lib.rs`: ツール定義5件を追加（ディスパッチは既存の汎用経路）。ツール定義の増加で `json!` が再帰上限に達したため `#![recursion_limit = "256"]` を追加。HTTP API は汎用ディスパッチのため変更なし。

## TDD の記録（red）

- core（`crates/studio-core/tests/effect_tests.rs`、8件）: `EffectSlot` / `TrackEffect` の import 解決失敗と `AddTrackEffect` バリアント不在でコンパイルエラー（red）。
- daw（`apps/daw/src/audio.rs` の `effect_tests`、5件）: 実装前は `eq3_bands_change_gain_by_configured_db` が「50Hz: expected 12 dB, measured 0.00 dB」で失敗（red）。`render_mix_applies_effects_only_to_their_track` も同様に失敗。
- 実装中に見つけた不具合: EQ の係数を暗記した Cookbook の閉形式で書いた初版は、ローシェルフの極が単位円外（約1.69）に出て発散し、測定が NaN になった。アナログ原型からの双一次変換で導出し直し、DC（ローシェルフ A^2）・中心（ピーキング A^2）・Nyquist のゲインで検算してから採用した（ADR-0023 と effects.rs のコメントに記録）。

## 単体テスト

- ワークスペース全体: 480 件 → **495 件**（新規15件: core 8件、daw render 経由 5件、effects DSP 単体 2件）、全て通過。
- `./scripts/verify.sh`: fmt / clippy / テストすべて OK。
- バイパス等価は `assert_eq!` のサンプル単位完全一致で固定（チェーン空・全バイパス・Reverb/Delay 素通り）。0dB EQ は浮動小数点誤差 1e-4 以内で一致。

## 実測検証（測定値）

EQ の帯域ゲイン（DSP 単体・render 経由とも同値、後半区間の RMS 比）:

| 測定 | 設定 | 実測 |
|---|---|---|
| ローシェルフ @50Hz | +12 dB | **+11.94 dB** |
| ローシェルフ @100Hz | +12 dB | **+11.10 dB** |
| ピーキング @1kHz | −12 dB | **−12.000 dB** |
| ハイシェルフ @10kHz | +12 dB | **+11.79 dB** |
| ハイシェルフ @8kHz | +12 dB | テストで ±1.5dB 以内を確認（レビュアーの独立計算では +11.35 dB） |

CLI フロー: 50Hz 正弦波（振幅 0.1）のプロジェクトを `daw --export-wav` で書き出し、Python で測定すると **+11.936 dB**（スクリプトと入力 JSON は `evidence/issue-0092/`）。

## 独立レビュー

- ラウンド1: **APPROVE**。受け入れ条件7件すべてをレビュアーが diff 全量と実測で確認（テスト 495 件通過も再実行で確認）。ブロッキング指摘なし。
- 非ブロッキング観察1件: 測定テストの代表周波数がイシュー記載（100Hz／8kHz）と異なる（50Hz／10kHz を使用）。レビュアーの独立計算では 100Hz +11.10dB・8kHz +11.35dB で仕様内だったが、記載と揃えるため `f927e0b` で 100Hz／8kHz のケースをテストに追加し、通過を確認した。

## 環境メモ

- 実施中、サービス再起動直後の一度だけ `verify.sh` が signalsmith-stretch の bindgen（libclang 探索）で失敗したが、同じコマンドの再実行で解消した（成果物とは無関係の環境側の揺れ）。

## Definition of Done チェック

- [x] 受け入れ条件をすべて満たす（ダンプ表現、範囲検証と失敗時不変、コマンド5種と Undo、旧形式互換、バイパス等価、EQ の測定、時間系の素通り明示）
- [x] TDD（red を記録）
- [x] `./scripts/verify.sh` 緑、CI 緑（run 45637834066、f927e0b）
- [x] 独立レビュー APPROVE（ラウンド1）
- [x] 判断は ADR-0023 に同じコミットで記録
