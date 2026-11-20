# 試験レポート： Issue #6 CI の整備（GitHub Actions）— 途中経過

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ）
- 対象コミット: `057e658`（verify 整備）, `fdcc672`（文書の文言修正）
- 状態: **未完了（needs-human で引き継ぎ）**。AC1 が権限のため達成できず、
  イシューは close しない。

## 受け入れ条件ごとの結果

### AC1: ワークフローがリポジトリに入り、Actions が緑 — ❌ 未達（権限ブロック）

- 確認方法（イシュー指定）: Actions の実行結果。
- `.github/workflows/ci.yml` を作成して push を試みたが、GitHub に拒否
  された: "refusing to allow an OAuth App to create or update workflow
  `.github/workflows/ci.yml` without `workflow` scope"。
  証拠: [evidence/issue-0006/workflow-push-rejected.txt](evidence/issue-0006/workflow-push-rejected.txt)
- ワークフローファイルの内容は Issue #6 のコメントに全文を残した。
  人間が workflow スコープ付きで再認証するか、Web UI からファイルを追加
  すれば、そのまま使える。
- ローカルコミットは巻き戻し済み（リモートと一致）。

### AC2: ローカルで同じ検証を一括実行する手順が明記され、実行できる — ✅

- `scripts/verify.sh` が fmt --check → clippy --workspace --all-targets
  -D warnings → cargo test --workspace を一括実行する。README に手順を明記。
- 実行結果: 終了コード 0、テスト合計24件通過。
  証拠: [evidence/issue-0006/verify-run.txt](evidence/issue-0006/verify-run.txt)
- あわせて、従来から残っていた studio-core の fmt 差分
  （`command.rs`、`command_tests.rs`、整形のみで振る舞いの変更なし）を
  解消した。これがないと fmt --check が常時失敗する。

## TDD の記録

- 本 PBI は設定と手順の整備であり、新規の振る舞いはない。検証は
  verify.sh の完走（終了コード 0）をもって確認した（AC2 の確認方法どおり）。

## コードレビュー

- ラウンド1: `CHANGES_REQUESTED`。指摘3件:
  1. ワークフローが無く AC1 が未達 → 権限ブロックのためエージェントでは
     解消できず、needs-human として引き継ぐ（本レポートとイシューの
     コメントに記録）。
  2. README と verify.sh の文言が「CI が既に動いている」ように読める →
     `fdcc672` で現状（未整備・対応中）を明記する文言に修正した。
  3. 試験レポートがない → 本レポートを作成した（ただし PBI は未完了の
     まま引き継ぐため、完了扱いにはしない）。

## 残作業（人間への引き継ぎ）

1. `.github/workflows/ci.yml` を追加する（内容は Issue #6 のコメントに
   全文あり）。方法は workflow スコープ付きの再認証、または Web UI。
2. Actions が緑になることを確認する。
3. 確認できたら本レポートを完了版に更新し、Issue #6 を close する。
