# 試験レポート： Issue #5 MCP サーバー

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ）
- 対象コミット: `1502a20`
- 環境: Yoko の VM。Rust: cargo 1.100.0

## 受け入れ条件ごとの結果

### AC1: MCP クライアントから tools 一覧にコマンド群が見える — ✅

- 確認方法（イシュー指定）: クライアントの出力を記録。
- 証拠: [evidence/issue-0005/mcp-session.txt](evidence/issue-0005/mcp-session.txt)
  の `tools/list` 応答に、コマンド6種（add_track / remove_track /
  add_clip / move_clip / remove_clip / set_clip_gain）と get_project の
  計7件が inputSchema 付きで並ぶ。テストでも同内容を確認。

### AC2: tool 呼び出しでクリップ追加等が実行され、状態ダンプに反映される — ✅

- 確認方法（イシュー指定）: API ダンプとの突き合わせ。
- テスト `tool_calls_execute_and_dump_reflects_them`: MCP 経由で
  add_track + add_clip を実行し、get_project のダンプが、同じコマンド列を
  コマンド層に直接適用したプロジェクトの `dump_json()` と**完全一致**
  することを確認した。HTTP API の `GET /project` は `dump_json()` を
  そのまま返す（PBI #4 で確認済み）ため、これが API ダンプとの同一性の
  突き合わせになる。
- 実セッションの証拠（同ファイル）でも add_track / add_clip の成功と
  ダンプへの反映を確認。

### AC3: 引数が不正な場合はエラーになり、状態は変わらない — ✅

- 確認方法（イシュー指定）: テスト。
- 存在しないトラックへの add_clip は `isError: true` でエラーメッセージを
  返し、引数の型が違う場合もエラー。どちらも前後のダンプが一致すること
  をテストで確認。

## TDD の記録

- red: テストを先に書き、`cargo test -p studio-mcp` がコンパイルエラーで
  失敗（E0432: `studio_mcp::McpServer` が未定義、バイナリ未存在、終了コード 101）。
- green: ライブラリ＋バイナリを実装し、テスト8件が通過。
  実装中のコンパイルエラー2件（`apply` の戻り値型の取り違え、型推論）は
  実装側の誤りとして修正した（テストは変更していない）。

## ゲート出力

- `cargo test --workspace`: 終了コード 0（合計24 passed: studio-api 8、
  studio-core 8、studio-mcp 8）。
- `cargo clippy -p studio-mcp --all-targets`: 終了コード 0。
- `cargo fmt --check -p studio-mcp`: 差分なし。
- CI: リポジトリにワークフローが未整備のため（PBI #6 で対応）、CI の緑の
  代わりに上記のローカル実行（fmt / clippy / test）を証拠とする。

## コードレビュー

- ラウンド1: **APPROVE**（指摘なし）。レビュアーは全 AC をテストと
  実セッション記録で再検証し、状態変更がコマンド層経由のみであること
  （直接変更の痕跡なし）を grep で確認済み。

## 判断の記録

- 新しい判断なし。プロトコルは MCP の最小サブセット（initialize /
  tools/list / tools/call / ping、stdio・改行区切り JSON-RPC）を
  serde / serde_json のみで実装した（ADR-0008 の方針どおり）。
