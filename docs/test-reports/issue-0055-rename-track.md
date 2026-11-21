# 試験レポート： Issue #55 トラック名を後から変更できる（RenameTrack コマンドと MCP 公開）

- 日付: 2026-11-22
- 実行者: エージェント（デリバリーループ、基本操作）
- 対象: コアに RenameTrack コマンドを追加し、MCP ツール `rename_track`
  として公開する。GUI のリネーム操作は後続 PBI（#56・#57）の担当。
- CI: https://github.com/Uncheese1Unko/rust-studio/actions/runs/44929761015
  （実装コミット `5fb130d`、緑）
- 環境: Yoko の VM（Linux, ヘッドレス）。本 PBI は画面で観測できる
  受け入れ条件を含まない（コア／MCP／API のみ）。ローカルビルドには
  `LIBCLANG_PATH=/usr/lib/llvm-18/lib` が必要。

## 経緯

- ユーザーが実機で両アプリを触り、「一般的な機能以前の問題」として
  基本操作の欠落を指摘した。本 PBI はそのうち「トラック名の変更」で、
  コアのコマンド自体が存在しなかった（リポジトリ全体に "Rename" の
  一致なし）ため、両アプリ GUI PBI（#56・#57）の先行として切り出した。
- 実施順はユーザーの委任を受けた監督側の決定（基本操作→共通基盤→
  DAW→動画編集）に従う。

## 受け入れ条件ごとの結果

### AC1: RenameTrack で名前が変わり、状態ダンプに新しい名前が現れる — ✅

- コア単体テスト `rename_track_changes_only_the_name`：名前だけが
  変わり、音量・クリップ・トラック数が不変であることと、ダンプに
  新しい名前が現れることを確認。
- MCP テスト `rename_track_tool_applies_via_tools_call`：tools/call
  経由で実行し、get_project のダンプに反映されることを確認。

### AC2: 存在しない track_id はエラーで状態不変 — ✅

- コア単体テスト `rename_track_unknown_id_keeps_state`：
  `CommandError::TrackNotFound(9999)` が返り、実行前後のダンプが
  完全一致することを確認。

### AC3: 空文字・空白のみの名前は InvalidRange で状態不変 — ✅

- コア単体テスト `rename_track_rejects_blank_name`：`""`、`" "`、
  `"  \t\n "` のいずれも `InvalidRange` となり、ダンプ不変を確認。
- MCP テストでも空白名が `isError: true` となり状態不変を確認。

### AC4: MCP ツール rename_track が tools/list に現れる — ✅

- MCP テスト `tools_list_exposes_all_commands` の期待一覧に
  `rename_track` を追加し、通過を確認。

### AC5: HTTP API からも同じコマンド JSON で実行できる — ✅

- API テスト `rename_track_command_applies_via_api`：API 側の変更なしで
  `{"type":"rename_track",...}` が 200 で適用され、ダンプに反映。
  空白名は 400 で状態不変。API は Command を汎用的にデシリアライズ
  するため追加対応が不要であることを裏付けた。

## TDD の記録

- ベースライン: 着手前に `cargo test --workspace` で 116 件通過・
  失敗 0 を確認。
- red: コアのテスト3件（`rename_track_changes_only_the_name`、
  `rename_track_unknown_id_keeps_state`、`rename_track_rejects_blank_name`）、
  MCP の tools/list 期待追加と `rename_track_tool_applies_via_tools_call`、
  API の `rename_track_command_applies_via_api` を先に書き、
  `cargo test -p studio-core --test command_tests rename_track` が
  `error[E0599]: no variant named 'RenameTrack' found for enum 'studio_core::Command'`
  で落ちることを確認した。
- green: `Command::RenameTrack { track_id, name }` と apply の実装
  （空白名は `InvalidRange("name must not be blank")`、検証は名前→
  トラック存在の順）、MCP のツール定義追加で全テスト通過。
  ワークスペース全体は 121 件通過・失敗 0（116 → 121、新規5件）。
  `./scripts/verify.sh`（fmt／clippy／test）も終了コード 0。

## コードレビュー

- 独立 code-reviewer（新規サブエージェント、イシュー番号と diff 範囲
  のみを渡した）ラウンド1で **APPROVE**、指摘なし。
- レビュアー自身も `cargo test -p studio-core -p studio-mcp -p studio-api`
  を実行し、44 件通過・失敗 0 を確認した。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red の記録は上記）
- [x] `cargo test` が 0 で終わった（飛ばし・弱体化なし）
- [x] 画面で観測できる受け入れ条件は本 PBI にはない（GUI は #56・#57）。
      状態ダンプ（get_project / HTTP API）で同じ状態が観測できることは
      MCP・API のテストで確認した
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断があれば ADR を同じ commit で足した（本 PBI での新規判断なし。
      色付けの判断は ADR-0013 で別途記録済み）
- [x] 試験レポートと証拠を置いた（本ファイル。証拠はテスト出力）
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
