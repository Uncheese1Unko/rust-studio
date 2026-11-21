# 試験レポート： Issue #28 DAW のピッチ表示に旧テーマの金色が残らなくなる（テーマ色 TEXT_SUB へ統一）

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ、フォローアップ PBI）
- 対象: #18 由来のピッチ表示の文字色（金色リテラル）をテーマの
  `TEXT_SUB` に統一する。#20 の試験レポート AC2 で「既知の残件」
  として明記された項目の解消
- CI: https://github.com/Uncheese1Unko/rust-studio/actions/runs/44909836831
  （実装コミット `8f27e74`、緑）
- 環境: Yoko の VM（Linux, ヘッドレス）。表示確認は Xvfb＋Mesa lavapipe
  （ソフトウェアレンダリング）。実ディスプレイでの確認は未検証。
  ローカルビルドには `LIBCLANG_PATH=/usr/lib/llvm-18/lib` が必要。

## 経緯

- #20 の締めで、ピッチ表示（`pitch: tone-440 +0.0 st`、画面左下の
  小さな文字）だけが旧来の金色リテラル `rgb(0.95, 0.75, 0.35)`
  （`apps/daw/src/main.rs`）のまま残っていることが開示された。
  #23 の時点で「後続 PBI で扱う」とされたが、#24〜#27 のいずれの
  受け入れ条件にも含まれず、分割から漏れていた。
- 本 PBI はその残件処理として起票し、EPIC #12 の sub-issue として
  リンクした（#13〜#27 と同じ並び）。

## 根本原因

#18 で追加されたピッチ表示の文字色が、テーマ適用の分割
（#23〜#27）のいずれの受け入れ条件にも含まれず、描画コードに
金色リテラルで残った。

## TDD の記録

- ベースライン: 着手前に `cargo test --workspace` を実行し、
  115 件通過・失敗 0 を確認した。
- red: テスト `pitch_label_color_is_theme_text_sub`
  （`apps/daw/src/main.rs` の `mod tests`）を先に書き、
  `cargo test -p daw pitch_label_color_is_theme_text_sub` が
  `error[E0425]: cannot find function 'pitch_label_color' in this scope`
  で落ちることを確認した（#23 と同じ、関数未定義のコンパイルエラー）。
- green: 色の決定を関数 `pitch_label_color() -> theme::Rgb`
  （`theme::TEXT_SUB` を返す）として追加し、view のピッチ表示が
  その返り値で描くように変えたところ、テストが通過した。
  あわせて旧リテラル `(0.95, 0.75, 0.35)` と等しくないことも
  テストで固定した。

## 変更内容

- commit: `8f27e74da1ae81c6d590cecf136ecca190383f55`（`apps/daw/src/main.rs` のみ、+21/-1）
- `pitch_label_color()` を新設し、ピッチ表示の `.color()` を
  旧リテラル `rgb(0.95, 0.75, 0.35)` から同関数の返り値
  （`theme::TEXT_SUB` = グレージュ `#A89880`）に置き換えた。
  色以外の描画（文言・位置・フォントサイズ 12）は変更していない。
- 色の選定根拠: デザイン文書「配色」表の役割づけでは小さい補助
  文字はグレージュ。同じ画面の補助文字（トランスポートの dB
  表示・時刻表示）は既に `theme::TEXT_SUB` を使っており、
  #20 レポートの推奨も `TEXT_SUB`。新規の色定数は作っていない
  （実装レベルの小判断として記録する。設計判断の変更はない）。
- 同種リテラルの棚卸し: `grep -rnE "rgb\([0-9]" apps/daw/src
  --include="*.rs"` で、theme.rs 以外の数値色リテラルは本件の
  1 件のみだった。`apps/video-editor/src/main.rs:615` に同色
  リテラル（切り抜き進捗表示）があるが、デザイン文書で動画編集
  へのテーマ展開は別途判断と決まっているため、本 PBI のスコープ
  外とし、イシュー本文に残件として記録した。

## 受け入れ条件ごとの結果

### AC1: ピッチ表示の文字色が theme::TEXT_SUB になっている — ✅

- 単体テスト `pitch_label_color_is_theme_text_sub` が通過。
- 実アプリのピクセル検証（証拠:
  [evidence/issue-0028/drive.sh](evidence/issue-0028/drive.sh),
  [shot-before.png](evidence/issue-0028/shot-before.png),
  [shot-after.png](evidence/issue-0028/shot-after.png),
  [pixel-check.txt](evidence/issue-0028/pixel-check.txt)）:
  ピッチ表示の領域（画面左下、x 30–300 / y 500–539）で、
  修正前は金色 (242, 191, 89) が 9 件・`TEXT_SUB` (168, 152, 128)
  が 0 件、修正後は金色 0 件・`TEXT_SUB` 9 件。字形のピクセル数が
  同じ 9 件で、色だけが変わったことも分かる。
- 修正後の全画面で、文言 `pitch: tone-220 +0.0 st` が同じ位置に
  グレージュで表示されることを目視でも確認した。

### AC2: DAW の描画コード（theme.rs を除く）に色の数値リテラルが残っていない — ✅

- `grep -rnE "rgb\([0-9]" apps/daw/src --include="*.rs" | grep -v
  "theme.rs"` が 0 件（grep の終了コード 1 = 該当なし）。

### AC3: 色以外の振る舞いが変わっていない — ✅

- `./scripts/verify.sh` が終了コード 0（fmt / clippy -D warnings /
  ワークスペース全体 116 件通過・失敗 0。ベースライン 115 件から
  新規テスト 1 件の増加のみ、削除・弱体化なし）。
- 状態ダンプ・コマンド層には手を入れていない（変更は view の
  文字色のみ）。ピッチ操作そのものの振る舞いは既存テスト
  （audio の pitch 系テスト群）が従来どおり通過している。

## コードレビュー

- 独立した `code-reviewer`（新規セッション）に、イシュー番号と
  diff 範囲（`31a3e38..8f27e74`）だけを渡して実施した。
- 結果: ラウンド1で **APPROVE**、指摘なし。レビュアー自身も
  `cargo test` を実行し、ワークスペース全体 116 件通過を確認した。
  AC1〜AC3 の充足、TDD の成立、スコープ遵守（video-editor 側は
  無変更、新規の色定数なし）が検証された。

## 完了の定義チェック（本イシュー分）

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red の記録は本レポート「TDD の記録」）
- [x] `cargo test` が 0 で終わった（飛ばし・弱体化なし）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を
      残した。状態ダンプでも同じ状態が観測できる（表示のみの
      変更で、状態は不変）
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断があれば ADR を同じ commit で足し、関係する文書と
      ルールを直した（新規の設計判断なし。色の選定はデザイン
      文書と #20 レポートの推奨に従った実装レベルの判断で、
      本レポートに記録した）
- [x] 試験レポートと証拠を置いた
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて
      閉じる（実施済み）
