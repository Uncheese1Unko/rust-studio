# 試験レポート： Issue #9 クリップの配置・移動・トリミング（コマンド経由）

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ、二周目）
- 対象コミット: `f710bdc`（TrimClip をコアに追加）, `17baa50`（GUI ドラッグ編集）,
  `c81bdbe`（レビュー指摘対応）
- CI: https://github.com/Uncheese1Unko/rust-studio/actions/runs/44866267993 (success)
- 環境: Yoko の VM（Linux, ヘッドレス）。実アプリ操作は Xvfb＋lavapipe 上で
  xdotool による合成マウス操作。実ディスプレイでの確認は未検証（#2 の申し送り）。

## 実装の概要

- コア: `Command::TrimClip { clip_id, new_start_ms, new_duration_ms }` を追加。
  検証（start >= 0、duration > 0、クリップ存在）を適用前に行い、失敗時は
  状態を変えない。配置は既存 AddClip、移動は既存 MoveClip を使う。
- MCP: 新コマンドを `trim_clip` ツールとして tools/list に公開（レビュー指摘1）。
  実行経路は従来どおり「tool 名 = コマンドの type タグ」の汎用デシリアライズ。
- GUI: `apps/video-editor/src/edit.rs`（新規）にポインタ→コマンド変換の
  純粋ロジック（ヒットテスト、ドラッグの候補コマンド生成、ダブルクリック
  判定、配置コマンド生成）を置き、`main.rs` はマウスをポーリングして
  候補コマンドを `apply` 経由で適用する。ドラッグ中は候補が変わるたびに
  適用し、解放時に状態ダンプを標準出力に出す。範囲外の候補は GUI で
  丸めず、そのままコマンド層に渡して弾かせる（AC2 の分担）。

## 受け入れ条件ごとの結果

### AC1: 配置・移動・トリミングがすべてコマンド経由で適用される — ✅

- 確認方法（イシュー指定）: 操作後の状態ダンプ。
- 実アプリ（xdotool 操作、再現手順は証拠 drive.sh）のダンプ遷移
  （証拠: [evidence/issue-0009/app-stdout-operations.txt](evidence/issue-0009/app-stdout-operations.txt)）:
  1. 移動: opening 0→1000ms（+84px ドラッグ = +1000ms、MoveClip）
  2. 右端トリム: interview duration 4000→3000ms（TrimClip）
  3. 左端トリム: interview 2500/3000→3500/2000、**end は 5500ms のまま**
     （レビュー指摘2を受けて追加した操作）
  4. 配置: ダブルクリックで clip-3（7500ms / 2000ms）が V1 に追加（AddClip）
- 最終スクリーンショット（[evidence/issue-0009/after-operations.png](evidence/issue-0009/after-operations.png)）
  のピクセル解析でも、最終ダンプどおりの位置にクリップが描かれている。
- 変換ロジックは単体テスト7件（ヒットテスト、総移動量、左右トリム、
  負候補、ダブルクリック、配置）で固定した。

### AC2: 不正な操作はコマンド層で弾かれ、状態が壊れない — ✅

- 確認方法（イシュー指定）: テストと実操作。
- テスト: TrimClip の不正範囲3ケース（負の start、duration 0、負の
  duration）と未知クリップで、いずれもエラーかつ `dump_json` 不変を固定。
  JSON 往復（`"type":"trim_clip"` タグ）も固定（レビュー指摘3）。
- 実操作: opening を左へ大きくドラッグすると、候補が負になった時点で
  コマンド層が `invalid range: new_start_ms must be >= 0` で弾き続け、
  クリップは最後の正常値 429ms に留まった（ダンプと
  `command rejected during drag: ...` の出力が証拠）。

## TDD の記録

- コア: TrimClip のテスト3件を先に書き、`error[E0599]: no variant named
  TrimClip` で落ちる（red）ことを確認してから実装した。
- GUI: edit.rs のテスト7件を先に書き、全関数 `unimplemented!()` の状態で
  7件すべてが落ちる（red）ことを確認してから実装した。
- レビュー指摘対応: MCP の tools/list 期待値に trim_clip を足し、
  `tools_list_exposes_all_commands` が落ちる（red）ことを確認してから
  ツール定義を追加した。あわせて tools/call での適用テストと、
  コアの JSON 往復テストを追加した。

## ゲート出力

- `./scripts/verify.sh`: 終了コード 0（テスト合計42件通過 =
  従来30件＋TrimClip コア3件＋JSON 往復1件＋edit 7件＋MCP 1件）。
- CI（コミット `c81bdbe` まで）: success（上記 URL）。

## コードレビュー

- ラウンド1: `CHANGES_REQUESTED`。指摘3件（いずれも妥当と判断し対応）:
  1. TrimClip が MCP tools/list に未公開で「GUI だけができる操作」に
     なっている（ADR-0004 違反）→ `trim_clip` ツールを追加し、
     tools/list のテストも追随させた（コミット `c81bdbe`）。
  2. 実アプリ証拠に左端トリムがない → drive.sh に左端ドラッグを追加し、
     end 不変が読めるダンプを証拠に残した。
  3. TrimClip の JSON シリアライズ可能性がテストで固定されていない →
     コアに JSON 往復テストを追加した。
- ラウンド2（新規レビュアー）: `APPROVE`。非ブロッキング観察として
  「HTTP API 経由の trim_clip 専用テストがない（到達可能性は汎用
  デシリアライズと JSON タグのテストで担保済み）」が挙がった。
  次に API テストへ触る機会に追加する。

## 観察（不具合ではない）

- 隣接したクリップ同士の端が 6px 程度まで近いと、ヒットテストは
  クリップ順に先勝ちするため、掴む位置によっては隣の端を掴むことがある
  （証拠採取の初回に実際に踏んだ）。M1 の範囲では仕様として受容し、
  拡大表示やスナップ等は将来の PBI 候補とする。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red を上記に記録）
- [x] `cargo test` が 0 で終わった（42件通過、飛ばし・弱体化なし）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断なし（ADR なし。MCP 公開は ADR-0004 の適用として本レポートに記録）
- [x] 試験レポートと証拠を置いて push し、CI が緑
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
