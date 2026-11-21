# 試験レポート： Issue #18 クリップのピッチ変更（性質を保つ）

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ、二周目）
- 対象コミット: `be4b5eb`（実装）, `f404ba4`（レビュー対応: MCP の call レベルテスト）
- CI: スタック push 後に記録する
- 環境: Yoko の VM（Linux, ヘッドレス）。音声出力デバイスはなく、
  聴取による確認は実機未検証（下記の残件参照）。

## 実装の概要

- コア: `Clip.pitch_semitones` と `Command::SetClipPitch`
  （±24 半音、有限値のみ。旧形式 JSON は 0.0 で読み込む）。
- 方式は ADR-0009 の確定どおり Signalsmith Stretch
  （`signalsmith-stretch` クレート）。フォルマント係数 1.0、
  compensate あり。ピッチ処理はアプリ側（daw の audio 層）に閉じ、
  `render_mix` はクリップごとに変更済み音声へ差し替える
  （長さ・サンプルレート・チャンネル数は不変）。
- GUI: ↑↓ キーで再生ヘッド位置の音声クリップを ±1 半音。
  MCP: `set_clip_pitch` ツール。

## 受け入れ条件ごとの結果

### AC1: ピッチ変更がコアのコマンドとして追加され、テストがある — ✅

- `set_clip_pitch_applies_and_validates`: 適用、JSON 往復、
  範囲外（24.5 / -25.0 / NaN）で状態不変、境界値 ±24 の通過、
  未知クリップのエラー、旧形式 JSON の読み込みを検証。

### AC2: GUI と MCP の両方から変更でき、ダンプに反映される — ✅

- GUI 駆動（証拠: [evidence/issue-0018/accept.sh](evidence/issue-0018/accept.sh),
  [gui-stdout.txt](evidence/issue-0018/gui-stdout.txt),
  [shot2-pitch.png](evidence/issue-0018/shot2-pitch.png)）:
  ↑ を2回押し、tone-220 の pitch_semitones が 0.0 → 1.0 → 2.0 と
  ダンプに反映された（tone-440 は 0.0 のまま）。
- MCP: tools/list への登録に加え、tools/call → get_project の
  ダンプ反映と、範囲外（99.0）の拒否で状態不変をテストで固定
  （レビュー対応 `f404ba4`）。手動でも stdio 経由で確認済み
  （set_clip_pitch 7.0 → ダンプに 7.0）。

### AC3: 声でピッチを上下させても性質が保たれる — ⚠️ 測定では ✅、聴取は残件

- 測定（証拠: [evidence/issue-0018/analysis.txt](evidence/issue-0018/analysis.txt),
  WAV 3本）: 合成の声らしい信号（F0=120Hz、フォルマント
  730/1090/2440Hz）で、F0 は +7 → 180Hz / -5 → 90Hz（理論値一致）、
  フォルマント包絡のピークは +7 で 719Hz と元の位置に留まった。
  単体テスト `pitch_shift_preserves_formant_position` でも
  +12 半音後の包絡ピークが 730Hz 近傍に留まることを固定している。
- 既知の特性: ADR-0009 の設定は狭帯域信号（純サイン）で約 -8.5dB の
  レベル低下を伴う（実測: 220Hz サイン +12 で RMS 0.283 → 0.108。
  compensate なしでは低下しない）。ADR の確定設定を勝手に変えず、
  既知事項として記録する。覆す場合は ADR-0009 側の判断事項。
- **残件（needs-human）**: 「実アプリでの聴取」は音声出力デバイスの
  ない環境では実施できない。実音声での聴取確認は、ユーザーが
  ADR-0009 決定時に実施済み（声×3＋ピアノ）。本 PBI の素材は
  実音声ではなく合成信号であり、聴取による最終確認は人間に残る。

### AC4: ミックス再生と組み合わさって破綻しない — ✅

- `render_mix_applies_clip_pitch`: ピッチ付き（440→880Hz）と
  無しのクリップの混在ミックスで、880 の支配と 440 の消滅を検証。
  配置・ゲイン・ミキサーの意味論は従来どおり（差し替えのみ）。

## TDD の記録

- コアの SetClipPitch テストを先に書き、`no field pitch_semitones` /
  `no variant SetClipPitch` のコンパイルエラー（red）を確認してから実装。
- エンジンの pitch テスト4件（オクターブ上下、長さ保持、
  フォルマント保持、ミックス統合）を先に書き、`pitch_shift` 未実装の
  エラー（red）を確認してから実装した。途中で測定法の誤り
  （長い窓の Goertzel は周波数の微小誤差で位相が崩れる）を検出し、
  50ms 分割平均の測定に直した。

## ゲート出力

- `./scripts/verify.sh`: 終了コード 0（90 件通過、実装時点）。
  ビルドには libclang が必要（bindgen）。ローカルでは
  `LIBCLANG_PATH=/usr/lib/llvm-18/lib` と
  `BINDGEN_EXTRA_CLANG_ARGS` を付けて実行した。

## コードレビュー

- ラウンド1: `CHANGES_REQUESTED`。コード本体（コマンド検証・状態
  不変、GUI/MCP の実動作、render_mix の意味論保持、フォルマント
  保持の測定）に機能面の欠陥は見つからなかった。
  - [Major] 試験レポートが存在しない → 本レポートで対応。
  - [Medium] MCP の tools/call レベルのテスト／証拠がない
    → テストを追加（`f404ba4`）。
  - [Minor] 聴取未検証を残件として明示すべき → 上記 AC3 に
    needs-human として明記。
- ラウンド2: 再審査で確認する。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
      （AC3 の聴取のみ needs-human として残件明記）
- [x] TDD で実装した（red を上記に記録）
- [x] `cargo test` が 0 で終わった（飛ばし・弱体化なし）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [ ] push の前に独立した `code-reviewer` が `APPROVE` を返した
      （ラウンド2の結果を記録する）
- [ ] この変更の commit で CI が緑（push 後に記録する）
- [x] 判断は ADR-0009（ユーザー決定済み）に従った。新規 ADR なし
- [x] 試験レポートと証拠を置いた
- [ ] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
      （push 後に行う）
