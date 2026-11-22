# 試験レポート： Issue #29 Undo/Redo の基盤（コマンド履歴と API/MCP の undo/redo）

- 日付: 2026-11-22
- 実行者: エージェント（デリバリーループ、共通基盤）
- 対象: studio-core にスナップショット方式の履歴機構を追加し、HTTP API と MCP から undo/redo できるようにする。GUI のキー操作は後続 PBI（#31）の担当。
- 実装コミット: `887a96e`
- CI: https://github.com/Uncheese1Unko/rust-studio/actions/runs/44958643105 （緑）
- 環境: Yoko の VM（Linux、ヘッドレス）。本 PBI は画面で観測できる受け入れ条件を含まない（コア／API／MCP のみ）。

## 受け入れ条件ごとの結果

### AC1: studio-core に履歴機構ができ、コマンド適用・undo・redo を単体テストで検証できる — ✅

- `crates/studio-core/src/history.rs` に `History`（Project、undo スタック、redo スタック）を追加した。適用前スナップショット方式で、上限は `HISTORY_LIMIT = 100` 件、超過時は古いものから捨てる。
- コアテスト6件を追加した。指定テスト `undo_restores_previous_project_state` を含み、redo の再適用、新規適用による redo 消去、上限100件も検証した。

### AC2: undo した状態が HTTP API の全量ダンプと MCP の get_project で観測できる — ✅

- API テスト `undo_redo_endpoints_restore_state` が、適用→undo→redo の各段階で `GET /project` のダンプ一致を確認した。
- MCP テスト `undo_redo_tools_restore_state` が、同じ流れを `get_project` で確認した。

### AC3: HTTP API に undo/redo の経路、MCP に undo/redo ツールが公開され、履歴が空ならエラーにならず何も起きない — ✅

- API に `POST /undo` / `POST /redo` を追加し、空履歴では 200 で `undone: false` / `redone: false` を返す。
- MCP の tools/list に `undo` / `redo` を追加し、空履歴の undo は `isError: false` で `undone: false` を返すことをテストした。レビュアーから、MCP の空 redo テストがない点が非ブロッキング観察として挙がったが、undo/redo は同一分岐であり、コアと API で空 redo は検証済みである。

### AC4: 失敗するコマンドは履歴に積まれない — ✅

- `History::apply` は成功時だけ適用前スナップショットを積む。失敗時はスナップショットへ戻し、状態と履歴の双方を不変に保つ。
- コアテスト `failed_command_is_not_recorded` が、空白名の RenameTrack が検証エラーになった後、undo 1回で初期状態へ戻り、2回目は false になることを確認した。

### AC5: プロジェクト JSON の形式は変わらない（履歴は永続化しない） — ✅

- `History` には Serialize を付けず、モデルと永続化形式は変更していない。既存の永続化テスト5件が無変更で通過した。

## TDD の記録

- ベースライン: 着手前のワークスペース全体は 149 件通過・失敗 0。
- red: `crates/studio-core/tests/history_tests.rs` を先に書き、`cargo test -p studio-core --test history_tests` が `error[E0432]: unresolved import studio_core::History` で落ちることを確認した。
- green: `History` の実装、API の History 保持と undo/redo 経路、MCP の History 保持と undo/redo ツール、永続化処理の共通化を行った。ワークスペース全体は 159 件通過・失敗 0（149 → 159、新規10件）。`./scripts/verify.sh` も終了コード 0。

## コードレビュー

- 独立 code-reviewer（新規サブエージェント、イシュー番号と diff 範囲 `b9392e1..887a96e` のみを渡した）ラウンド1で **APPROVE**。
- 是正指摘なし。非ブロッキング観察2件（MCP の空 redo テストがないこと、無動作の undo/redo でもプロジェクトファイルを書き直すこと）は上記のとおり記録し、本 PBI の受け入れ条件を損なわないため対応不要と判断した。
- レビュアー自身もワークスペース全体 159 件通過、fmt と clippy の通過を確認した。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red の記録は上記）
- [x] `cargo test` が 0 で終わった（飛ばし・弱体化なし）
- [x] 画面で観測できる受け入れ条件は本 PBI にはない（GUI は #31）。状態ダンプで同じ状態が観測できることは API・MCP のテストで確認した
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断があれば ADR を同じ commit で足した（本 PBI での新規判断なし。方式はイシューの実装方針どおり）
- [x] 試験レポートを `docs/test-reports/` に置いた（本ファイル。証拠はテスト出力）
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
