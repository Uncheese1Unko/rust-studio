# 試験レポート： Issue #6 CI の整備（GitHub Actions）— 完了版

- 日付: 2026-11-21（途中経過版と同日、ユーザーのワークフロー追加を受けて完了版に更新）
- 実行者: エージェント（デリバリーループ）
- 対象コミット: `057e658`（verify 整備）, `fdcc672`（文書の文言修正）,
  `2cc472c`（ワークフロー追加、ユーザーによる）
- 状態: **完了**。AC1・AC2 とも達成し、Issue #6 を close する。

## 経緯

- 一周目では `.github/workflows/ci.yml` の push が OAuth スコープ不足で
  拒否され、AC1 未達のまま needs-human で引き継いだ（途中経過版レポート）。
- その後ユーザーがワークフローファイルを自分で追加し（コミット `2cc472c`
  "Add CI workflow for Rust project"）、GitHub Actions が成功した。
  本レポートはその確認をもって完了版とする。

## 受け入れ条件ごとの結果

### AC1: ワークフローがリポジトリに入り、Actions が緑 — ✅

- 確認方法（イシュー指定）: Actions の実行結果。
- `.github/workflows/ci.yml` がリポジトリに存在する（fmt --check →
  clippy --workspace --all-targets -D warnings → cargo test --workspace）。
- ワークフロー追加コミット `2cc472c` の Actions 実行が success:
  https://github.com/Uncheese1Unko/rust-studio/actions/runs/44859282166
- さらに最新コミット `fd7f1bb`（ADR-0009）の実行も success:
  https://github.com/Uncheese1Unko/rust-studio/actions/runs/44862818782
- 以前の push 拒否の記録は経緯として残す:
  [evidence/issue-0006/workflow-push-rejected.txt](evidence/issue-0006/workflow-push-rejected.txt)

### AC2: ローカルで同じ検証を一括実行する手順が明記され、実行できる — ✅

- `scripts/verify.sh` が fmt --check → clippy --workspace --all-targets
  -D warnings → cargo test --workspace を一括実行する。README に手順を明記。
- 完了版更新時点の再実行結果: 終了コード 0、テスト合計24件通過。
  証拠: [evidence/issue-0006/verify-run-completed.txt](evidence/issue-0006/verify-run-completed.txt)
- あわせて、従来から残っていた studio-core の fmt 差分
  （`command.rs`、`command_tests.rs`、整形のみで振る舞いの変更なし）を
  解消済み。これがないと fmt --check が常時失敗する。

## TDD の記録

- 本 PBI は設定と手順の整備であり、新規の振る舞いはない。検証は
  verify.sh の完走（終了コード 0）と Actions の緑をもって確認した
  （AC の確認方法どおり）。

## コードレビュー

- ラウンド1: `CHANGES_REQUESTED`。指摘3件とその解消:
  1. ワークフローが無く AC1 が未達 → ユーザーが `2cc472c` で追加し
     Actions が緑になったことで解消（上記 AC1）。
  2. README と verify.sh の文言が「CI が既に動いている」ように読める →
     `fdcc672` で現状を明記する文言に修正済み。
  3. 試験レポートがない → 本レポート（完了版）で解消。
- 本完了版の変更は文書のみ（レポート更新と証拠追加）であり、コード変更を
  含まないため、追加のレビューラウンドは行っていない。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（本 PBI は設定整備で新規の振る舞いなし。確認方法は
      verify 完走と Actions の緑。上記「TDD の記録」参照）
- [x] `cargo test` が 0 で終わった（verify.sh 経由、24件通過）
- [x] 画面で観測できる受け入れ条件はない（CI の結果は Actions の URL で確認）
- [x] 独立レビューを受けた（ラウンド1 の指摘3件はすべて解消。本更新は文書のみ）
- [x] この変更の commit で CI が緑（完了コメントに URL を記載）
- [x] 判断なし（ADR なし）
- [x] 試験レポートと証拠を置いて push し、CI が緑
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
