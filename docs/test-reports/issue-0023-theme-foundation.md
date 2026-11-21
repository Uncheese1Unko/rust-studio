# 試験レポート： Issue #23 DAW テーマ基盤（theme.rs 集約とライトテーマ化）

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ、#20 デザイン確定後の実装編）
- 対象コミット: `b62ab6d`（実装）
- CI: 緑（実装コミットで確認）: https://github.com/Uncheese1Unko/rust-studio/actions/runs/44897352616
- 環境: Yoko の VM（Linux, ヘッドレス）。表示確認は Xvfb＋Mesa lavapipe
  （ソフトウェアレンダリング）。実ディスプレイでの確認は未検証。
  ローカルビルドには `LIBCLANG_PATH=/usr/lib/llvm-18/lib` が必要
  （ADR-0009 のビルド要件。VM 初期化で消えていた libclang と
  mesa-vulkan-drivers を復旧して実施）。

## 実装の概要

- `apps/daw/src/theme.rs` を新設し、デザイン文書
  （`docs/design/daw-visual-design.md`）の配色表の全10色を定数化した。
  あわせてパネルラベル（`01 TRANSPORT`〜`04 SAMPLER`）、ラベル／
  現在値の文字サイズ、角丸半径の定数も置いた。
- `main.rs` の描画で、背景・ルーラー帯・トラック行・トラックヘッダー・
  ミキサー帯の面と区切り線、通常文字色（トラック名・クリップ名・
  秒ラベル）を theme 定数経由に置き換えた。パネルラベルは
  `02 TIMELINE`（ルーラーのヘッダー部）、`03 MIXER`（ミキサー帯の
  左上）、`04 SAMPLER`（鍵盤の上）を描画する。
- 状態変更には一切触れていない（描画のみ）。テーマは DAW アプリ内に
  閉じている（ADR-0012）。

## 受け入れ条件ごとの結果

### AC1: theme.rs に配色表の全色が定数として定義されている — ✅

- 単体テスト `theme_palette_matches_design_doc` が、10色すべてに
  ついて定数を HEX 表記に戻して配色表と突き合わせ、通過した。
  `panel_labels_follow_design_doc` でラベル文字列も固定した。

### AC2: 背景・パネル面が theme 定数経由で、main.rs に背景・パネル部の色リテラルが残っていない — ✅

- `grep -n "rgb(" apps/daw/src/main.rs` の出力を証拠
  （[evidence/issue-0023/grep-rgb-main.txt](evidence/issue-0023/grep-rgb-main.txt)）
  に残した。残るリテラルは波形・ミキサー内部・ピッチ表示・
  サンプラー鍵盤など、後続 PBI（#24〜#26）の範囲のものだけであり、
  背景・パネル部には残っていない（レビュアーも同じ grep で確認）。

### AC3: 実アプリで背景がクリーム、パネルがペールクリーム＋細線区切りで表示される — ✅

- 実アプリ駆動（証拠: [evidence/issue-0023/drive.sh](evidence/issue-0023/drive.sh),
  [shot1-theme.png](evidence/issue-0023/shot1-theme.png)）:
  スクリーンショットでライトテーマの画面を確認した。
- ピクセル検証（[pixel-check.txt](evidence/issue-0023/pixel-check.txt)）:
  背景 `#FBF4EC`、ルーラー帯・トラックヘッダー `#F3E7D9`、
  パネル面 `#FFFDF9` がデザイン文書の色値と一致した。区切り線
  `#E0D2C2` も画像内の出現色として確認した。

### AC4: 既存の操作が従来どおり動き、状態ダンプでも同じ状態が観測できる — ✅

- `cargo test` 全106件通過（変更前後で操作系のテストは不変）。
  起動時の状態ダンプ（[gui-stdout.txt](evidence/issue-0023/gui-stdout.txt)）
  に要素一覧とプロジェクト JSON が従来どおり出ている。
  描画のみの変更で、コマンド経路には触れていない。

## TDD の記録

- `theme::tests::theme_palette_matches_design_doc` と
  `panel_labels_follow_design_doc` を先に書き、theme 定数が未定義の
  ためコンパイルエラー（E0425: cannot find value `BACKGROUND` ほか
  14件）で落ちること（red）を確認してから定数を実装した。

## ゲート出力

- `./scripts/verify.sh`: 終了コード 0（fmt / clippy -D warnings /
  ワークスペース全体 106 件通過、daw は 43 件）。
- clippy の dead_code 対策として、配色表の全色を先に定義する土台で
  あることをコメントし、`mod theme` に `#[allow(dead_code)]` を
  付けた（アクセント系・トランスポート系は #24〜#27 で使用）。

## コードレビュー

- ラウンド1: `APPROVE`（指摘なし）。
  - 非ブロッキング観察として、`01 TRANSPORT` ラベルは定数のみで
    未描画（トランスポート表示は #27 で新設されるため）である点と、
    プレイヘッド・クリップ等の旧色は後続 PBI の範囲として意図的に
    残っている点が記録された。

## 判断の記録（AI が決めた実装レベルの小判断）

- パネルラベル `01 TRANSPORT` は本 PBI では描画しない。現行画面に
  トランスポートのパネルが存在せず、表示は #27 で新設されるため。
  定数とテストによる固定までを本 PBI の範囲とした。
- ラベルの配置: TIMELINE はルーラー帯のヘッダー部、MIXER は
  ミキサー帯先頭の左上、SAMPLER は鍵盤の上。デザイン文書の
  「パネル左上に小さく」に沿う範囲での配置判断。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red を上記に記録）
- [x] `cargo test` が 0 で終わった（飛ばし・弱体化なし）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
      （ラウンド1で APPROVE）
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断があれば ADR を同じ commit で足し、関係する文書とルールを
      直した（設計判断なし。小判断は本レポートに記録）
- [x] 試験レポートと証拠を置いた
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
      （実施済み）
