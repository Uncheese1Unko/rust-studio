# 試験レポート： Issue #4 HTTP/JSON API（コマンド実行と状態ダンプ）

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ）
- 対象コミット: `1ef6237`（新設）, `5f0b077`（レビュー指摘対応1）, `12348a0`（レビュー指摘対応2）
- 環境: Yoko の VM。Rust: cargo 1.100.0

## 受け入れ条件ごとの結果

### AC1: POST でコマンド JSON を送ると適用され、成否が返る — ✅

- 確認方法（イシュー指定）: curl の記録を残す。
- 証拠: [evidence/issue-0004/curl-session.txt](evidence/issue-0004/curl-session.txt)
（実行コマンド逐語＋実出力）。`add_track` → `{"created_id":1,"ok":true}`、
`add_clip` → `{"created_id":2,"ok":true}`。

### AC2: GET で状態の全量ダンプが返り、コマンドの効果が現れる — ✅

- 同じ証拠ファイルで、POST 後の `GET /project` にトラック V1 と
クリップ op（duration_ms 1500）が現れることを確認。

### AC3: 失敗するコマンドは状態を変えず、エラー内容を返す — ✅

- 存在しないトラックへの `add_clip` → `400 {"error":"track 999 not found","ok":false}`。
その後のダンプは失敗前と同一（`next_id` も進んでいない）。
- 不正な JSON も 400 で、状態は不変（テストで確認）。

## TDD の記録

| # | red | green |
|---|---|---|
| 1 | テストを先に書き、`cargo test -p studio-api` がコンパイルエラーで失敗（E0432: `handle_request` / `ApiState` / `Request` が未定義、終了コード 101） | ライブラリ＋バイナリを実装し、API テスト6件が通過 |
| 2 | レビューで発見・再現: 応答を読まず RST で切断するクライアント1件でサーバープロセスが終了する（`ConnectionReset` を `serve()` が伝播）。手元でも Python スクリプトで再現（サーバー死亡を確認） | 接続処理を `handle_connection` に分け、接続単位のエラーは記録してループを継続するよう修正。再現スクリプトで生存を確認し、回帰テスト `server_survives_abruptly_closed_connections` を追加 |
| 3 | レビュー round2 で発見・再現: 何も送らない接続を保持すると、逐次処理のため後続リクエストが無期限にブロックされる（再現スクリプトで 3 秒タイムアウト＝WEDGED を確認） | `read_request` 全体に10秒の期限を設け、超過した接続は閉じて次へ進むよう修正。再現スクリプトで「期限後に後続リクエストが処理される」ことを確認し、回帰テスト `server_does_not_wedge_on_an_idle_connection` を追加 |

バグはいずれも本 PBI の範囲内のため、bug-handling ルールに従い
本 PBI 内で修正した（バグ PBI は作らず、経緯をここに記録）。

## ゲート出力

- `cargo test --workspace`: 終了コード 0（合計16 passed: studio-api 8、
studio-core 8）。飛ばした・弱めたテストはない。
- `cargo clippy -p studio-api -p studio-core --all-targets`: 終了コード 0。
- `cargo fmt --check`: 本 PBI の変更ファイルは差分なし
（`command.rs` 等の既存差分は PBI #6 で解消予定）。
- CI: リポジトリにワークフローが未整備のため（PBI #6 で対応）、CI の緑の
代わりに上記のローカル実行（fmt / clippy / test）を証拠とする。

## コードレビュー

- ラウンド1: `CHANGES_REQUESTED`。指摘2件 — (1) RST 切断でサーバーが
死ぬ不具合、(2) curl 証拠がプレースホルダ表記で再実行できない。
両方とも再現・修正し、証拠は逐語に差し替えた。
- ラウンド2: `CHANGES_REQUESTED`。指摘1件 — アイドル接続でサーバーが
固まる不具合（同種の別バリアント）。再現・修正した。minor note
（クエリ文字列で 404 になる）も同コミットで対応した。
- ラウンド3: **APPROVE**（レビュアーが実バイナリへの curl と
テスト再実行で全 AC を再検証済み）。

## 判断の記録

- ADR-0008（決定者：エージェント）: HTTP API は std のみの薄いラッパー
として実装する。ライブラリ＋バイナリ構成とし、「アプリ内蔵か
独立プロセスか」は未決のまま残した。
