# 試験レポート： Issue #3 プロジェクトの保存・読込（JSON 永続化）

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ）
- 対象コミット: `5ad3c8a`
- 環境: Yoko の VM。Rust: cargo 1.100.0

## 受け入れ条件ごとの結果

### AC1: Project を JSON に保存し、読み戻すと同一の状態になる — ✅

- 確認方法（イシュー指定）: コアのラウンドトリップテスト。
- `save_and_load_round_trip_restores_identical_state` が通過。コマンド層で
  組み立てた Project（トラック＋クリップ）を保存し、読み戻して
  `PartialEq`（`next_id` を含む）と `dump_json()` の一致を確認した。

### AC2: 壊れた JSON を読むとエラーになり、現在の状態は変わらない — ✅

- 確認方法（イシュー指定）: テスト。
- `load_of_corrupt_json_errors`、`failed_load_leaves_existing_state_unchanged`、
  `load_of_missing_file_errors` が通過。読み込みは新しい Project を返す
  方式のため、失敗時に既存の状態へ触れる経路が存在しないことを、
  既存プロジェクトのダンプ不変で確認した。

## TDD の記録

| # | red | green |
|---|---|---|
| 1 | テストを先に書き、`cargo test -p studio-core --test persistence_tests` がコンパイルエラーで失敗（E0599: `save_to_path` / `load_from_path` が存在しない、終了コード 101） | `model.rs` に `save_to_path` / `load_from_path` と `PersistenceError` を実装し、新規4件＋既存4件の全8件が通過 |

## ゲート出力

- `cargo test --workspace`: 終了コード 0（studio-core 8 passed、他 0 件）。
- `cargo clippy -p studio-core --all-targets`: 終了コード 0、指摘なし。
- `cargo fmt --check`: 本 PBI の変更ファイルは差分なし。なお `model.rs` 内に
  あった既存の fmt 差分（`Project::clip` の折り返し）は、タッチセット内の
  ファイルのため本変更で解消した。`command.rs` と `tests/command_tests.rs`
  の既存 fmt 差分はタッチセット外のため触らず、PBI #6 で解消する。
- CI: リポジトリにワークフローが未整備のため（PBI #6 で対応）、CI の緑の
  代わりに上記のローカル実行（fmt / clippy / test）を証拠とする。

## コードレビュー

- ラウンド1: **APPROVE**（レビュアーがテストを再実行し、AC1/AC2 の充足と
  ゲート不変を確認済み）。

## スコープ外の変更について

- `crates/studio-core/src/lib.rs` の1行（`PersistenceError` の re-export）は
  イシューの「触るファイル」に含まれていないが、新しい公開 API を
  既存の re-export 群と揃えるために必要な変更として実施し、ここに明記する
  （レビュアーも妥当と判断）。
