# 試験レポート： Issue #61 バグ: History::apply_transient の失敗時原子性がコマンド実装の契約依存になっている

- 日付: 2026-11-22
- 実行者: エージェント（並行レーンC、コア担当）
- 対象: `crates/studio-core/src/history.rs` の `History::apply_transient` に、失敗時の適用前スナップショット復元（`History::apply` と同じ二重ガード）を追加する。
- 実装コミット: `fed809f`（レビュー対象は同一内容の `71ad919`。その後 main 追随のため rebase した）
- CI: 緑 https://github.com/Uncheese1Unko/rust-studio/actions/runs/45003190702
- 環境: Yoko の VM（Linux、ヘッドレス）。画面で観測できる受け入れ条件はない（コアの単体テストで確認）。

## 受け入れ条件ごとの結果

### AC1: `apply_transient` が失敗時にプロジェクト状態を変更しない — ✅

- 再現には「変更してから失敗する」コマンドが必要だが、実コマンドは全て「検証 → 変更」の順で失敗時に状態を変えない。そこでテスト専用コマンド `Command::MutateThenFail`（`#[cfg(test)]`、本番ビルドには含まれない）を追加した。適用するとトラックを追加し id を消費してから `Err` を返す。
- 単体テスト `history::tests::transient_failure_restores_pre_apply_state`（`crates/studio-core/src/history.rs`）で、失敗後の `dump_json()` が適用前と一致することを確認した。
- 補助テスト `history::tests::transient_failure_leaves_history_untouched` で、失敗時に undo/redo の可否と redo で戻れる状態も変わらないことを確認した。なお独立レビュアーの検証では、このテストはガードを外した状態でも通過した（redo が現在状態を丸ごと差し替えるため）。履歴不変の確認用としては有効だが、原子性の回帰検出は第1のテストが担う。

### AC2: 既存のジェスチャ記録（ドラッグ／ミキサー）の振る舞いが変わらない — ✅

- `./scripts/verify.sh` 緑（fmt / clippy -D warnings / ワークスペース全テスト）。studio-core は修正前 40 件 → 42 件（新規2件）全通過。#31 の transient 関連テスト（`transient_applies_with_recorded_snapshot_undo_as_one_step`、`transient_apply_alone_does_not_record_history`、`record_undo_snapshot_skips_state_equal_to_current`）を含む既存テスト全件が通過した。

## TDD の記録

- ベースライン: 変更前の `cargo test -p studio-core` は 40 件通過（command 26、history 9、persistence 5）。
- red: テスト専用コマンド `MutateThenFail` と単体テスト2件を先に追加し、`transient_failure_restores_pre_apply_state` が期待した理由で落ちることを確認した。失敗内容: 失敗後も `mutated` トラックが残り、`next_id` が 3 に進んでいた（適用前ダンプと不一致）。もう1件（履歴不変のテスト）は修正前から通過（ガード用）。
- green: `apply_transient` が適用前スナップショットを取り、失敗時に復元してから結果を返すよう変更し、42 件全通過を確認した。あわせて `apply_transient` の doc コメントを「コマンド層の契約どおり」から二重ガードの説明に更新した。

## 実装の要点

- `apply_transient` は成功時に履歴へ積まない性質はそのまま。失敗時のみ復元する。成功パスにもスナップショットの clone コストが乗るが、`apply` が既に同じ方式であり、ドラッグの逐次適用でも支配的なコストではないと判断した（計測はしていない。問題になれば差分適用への変更を別 PBI で検討する）。
- スコープ外の変更はしていない：各コマンドの検証順序、transient 経路の API 形状は不変。`MutateThenFail` は `#[cfg(test)]` のため公開 API に現れない。

## コードレビュー

- 独立 code-reviewer（新規サブエージェント、イシュー番号と diff 範囲 `71ad919^..71ad919` のみを渡した）が1ラウンドで **APPROVE**。
- レビュアーは `/tmp` の一時コピーでガードだけを旧実装に戻し、再現テストが実際に落ちること（`mutated` トラック残留と `next_id: 3`）を自分で検証した。
- 非ブロッキングの指摘2件（いずれも受け入れ条件を満たした範囲）：
  1. 補助テスト（履歴不変）はガードなしでも通過し、回帰検出力はほぼない。本体の原子性は再現テストが担う。今後このテストだけを根拠に原子性を主張しないこと。→ AC1 の記述に反映した。
  2. 成功時も毎回 `Project` を clone するコスト増。イシューの実装方針が明示したトレードオフであり、方針どおりのため対応なし（本レポートの「実装の要点」にも記録済み）。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red を上記「TDD の記録」に残した）
- [x] `cargo test` が 0 で終わった（`./scripts/verify.sh` 緑、飛ばし・弱めた確認なし）
- [ ] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した — 対象外（コアの履歴機構のみの変更で、画面で観測できる受け入れ条件はない。close コメントに明記する）
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した（1ラウンド。指摘と対応は上記「コードレビュー」）
- [x] この変更の commit で CI が緑（最後まで見た。上記 URL）
- [x] 判断があれば ADR を同じ commit で足した（新規の設計判断なし。方式はイシューの実装方針どおり `apply` と同じ二重ガード）
- [x] 試験レポートを `docs/test-reports/` に置いて push し、CI が緑（レポート自体の追補 commit の CI も確認してから close する）
- [x] 全部にチェックを入れ、commit・CI の URL・試験レポートへのリンクをコメントに書いて閉じた
