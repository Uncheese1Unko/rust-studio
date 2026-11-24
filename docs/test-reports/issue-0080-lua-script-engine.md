# 試験レポート: #80 Lua 実行基盤

対象 PBI: #80「Lua 実行基盤を組み込む（サンドボックス付きでスクリプトを実行できる）」
実施日: 2026-11-24
実装コミット: `1e17dd4`
ベース: `22b1a6c`

## 実装内容

- `apps/video-editor/src/script.rs` を新設。`ScriptEngine` が実行ごとに Lua 状態を作り、スクリプト文字列／ファイルを実行して、戻り値（`ScriptValue`: Nil／Bool／Number／Text）か構造化エラー（`ScriptError { kind, message }`、種別は Io／Syntax／Runtime／InstructionLimit）を返す。スカラー以外の戻り値は Runtime エラーとして返す。
- ランタイムは ADR-0019 の確定どおり **Lua 5.1 系（mlua 0.11.6、`features = ["lua51", "vendored"]`）**。`apps/video-editor/Cargo.toml` に依存を追加。
- サンドボックス:
  - 標準ライブラリは `StdLib::TABLE | STRING | MATH` の最小集合のみ読み込む。mlua の `ALL_SAFE` は io／os を含むため使わない。`os`／`io`／`package`／`debug` は存在しない。
  - ベースライブラリは常に読み込まれるため、到達経路になる関数は個別に nil 化する：ファイル読み込み系（`dofile`／`loadfile`／`load`／`loadstring`／`require`）と、エラー捕捉系（`pcall`／`xpcall`／`coroutine`）。後者はレビュー round 1 指摘への対応（下記）。
  - メモリ上限 64MB（`set_memory_limit`）。
- 最小 API（内部 API。公開互換形状は #81／#83 で確定する）: 読み取り専用グローバル `clip`（name／duration_ms／source、素材なしは nil）と `params`（数値・真偽・文字列のテーブル）。プロジェクト状態を書き換える経路は持たない（ADR-0004）。
- 暴走打ち切り: 命令数フック（2,048 命令間隔）で累計を数え、上限（既定 10,000,000、`with_instruction_limit` で変更可）超過でエラーを投げて割り込む。超過フラグは実行の成否にかかわらず検査し、上限超過は必ず `InstructionLimit` として返す。
- `apps/video-editor/src/main.rs` には mod 宣言のみ追加。利用側（#81／#83）が後続のため、現時点ではテストからのみ使う旨をコメントし `#[allow(dead_code)]` を付けた。

## 受け入れ条件の確認

- [x] スクリプトファイル／文字列を実行でき、戻り値／エラーが構造化されて返る — テスト `run_file_executes_script_file`、`returns_number_result`、`returns_text_bool_and_nil_results`、`syntax_error_is_structured`、`runtime_error_is_structured`、`run_file_missing_is_io_error`、`non_scalar_return_is_error`
- [x] サンドボックスが効いている（ファイル読み書き・OS コマンド・ネットワークへ到達できない） — テスト `sandbox_has_no_os_io_package_debug`、`sandbox_blocks_reach_attempts`（os.execute／io.open／require／loadfile／dofile／load の到達試行がすべて失敗）
- [x] 最小 API（クリップ情報の取得、パラメータの受け取り） — テスト `clip_info_is_exposed`、`clip_source_is_nil_when_absent`、`params_are_exposed`
- [x] 暴走スクリプトを打ち切れる — テスト `infinite_loop_is_interrupted_by_instruction_limit`、`pcall_wrapped_infinite_loop_terminates_with_error`、`long_loop_over_limit_is_instruction_limit_not_success`、`sandbox_has_no_error_catching_primitives`

画面で観測できる受け入れ条件は無い（基盤モジュールのため）ので、実アプリ駆動とスクリーンショットは対象外。状態ダンプ観測も、プロジェクト状態を持たないため対象外。

## TDD の記録（red）

1. 初期 red: テスト13件を先に書き、`run_source`／`run_file` が `unimplemented!()` のため全件が `not implemented` の panic で失敗することを確認した（0 passed / 13 failed）。実装後に green。
2. レビュー round 1 指摘の修正時の red:
   - レビュアーのプローブ `while true do pcall(function() while true do end end) end`（上限 20,000）が 300 秒タイムアウトまで終了しないことを確認（上限エラーが通常の Lua エラーで `pcall` に捕捉され、打ち切りが回避される）。
   - 追加テスト `sandbox_has_no_error_catching_primitives` が失敗すること（`pcall` 等が存在する）を確認した。
   - 修正（捕捉系の nil 化＋超過フラグを成否不問で検査）後に全件 green。プローブは即座にエラーで終了する。

## 単体テスト

- `cargo test -p video-editor`: 90 件 → 106 件（新規16件）。
- ワークスペース全体: 383 件 → 399 件、全て通過。
- `./scripts/verify.sh`: fmt / clippy / テストすべて OK。

## 独立レビュー

- ラウンド1: CHANGES_REQUESTED。指摘2件:
  1. （重大）命令数上限のエラーが `pcall`／`coroutine.resume` で捕捉でき、暴走スクリプトが止まらない（プローブで実証）。→ エラー捕捉系（`pcall`／`xpcall`／`coroutine`）をサンドボックスから外すことで対応。スクリプト側のエラー処理は構造化エラーとしてホスト側が担う設計とする。
  2. （中）上限超過後にスクリプトが正常終了すると成功として返る（`Ok` 分岐で超過フラグを見ていない）。→ 成否にかかわらず超過フラグを検査するよう修正。
- ラウンド2: APPROVE（新規レビュアー）。メタメソッド内ループ・深い再帰・`string.rep` のメモリ爆弾への追加プローブでも、上限／メモリ制限が効くことを確認済み。

## 判断の記録

- ランタイム選定（Lua 5.1 系、mlua + vendored）は ADR-0019 の採用済み決定に従った。本 PBI で新たな設計判断はしていない。
- 実装レベルの判断: サンドボックスから `pcall`／`xpcall`／`coroutine` を外した（理由は上記）。**#83 への申し送り**: 実在の .anm スクリプトは `pcall` を使う場合があるため、互換プロファイルでは「上限超過時に再送出する保護付き pcall」をホスト関数として提供する等、基盤の既定とは別に再評価すること。

## Definition of Done チェック

- [x] 受け入れ条件をすべて、書いてある確認方法（単体テスト）で確かめた
- [x] TDD（red を記録）
- [x] `cargo test` が 0 で終わった（ワークスペース 399 件）
- [x] 画面で観測できる受け入れ条件は無い（基盤モジュールのため対象外）
- [x] 独立レビュー APPROVE（2ラウンド）
- [x] 実装コミット `1e17dd4` の CI が緑
- [x] 判断は ADR-0019 に既存。本レポートに判断と申し送りを記録
- [x] 試験レポートを配置し push、CI 緑を確認する
