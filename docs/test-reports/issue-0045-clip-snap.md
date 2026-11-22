# 試験レポート： Issue #45 動画編集のクリップ移動・トリムがスナップする

- 日付: 2026-11-22
- 実行者: エージェント（デリバリーループ、並行レーンB＝動画編集 担当）
- 対象: クリップの移動・トリム時に、他クリップの端・再生ヘッド・
  0ms・1秒グリッドへ吸着するスナップを実装する。Alt 押下中は無効。
- 実装コミット: `9e8b421`（rebase 前 `1a92aa1`）
- CI: 緑 https://github.com/Uncheese1Unko/rust-studio/actions/runs/45005400843
- 環境: Yoko の VM（Linux、ヘッドレス）。表示確認は Xvfb（:92）＋Mesa
  lavapipe（ソフトウェアレンダリング）。**実ディスプレイでの確認は未検証。**
- テスト件数: ベースライン 219 件 → 225 件（新規6件、すべて
  `apps/video-editor/src/edit.rs` の単体テスト）。削除・弱体化なし。

## 受け入れ条件ごとの結果

### AC1: 移動すると端が他クリップの端・再生ヘッド・0ms・1秒グリッドに（8px 以内）吸着する — ✅

- スナップは `edit::snap_edge`（純粋関数）: 対象クリップ以外の全
  クリップの開始端・終端、再生ヘッド、0ms、前後の1秒グリッドを
  候補とし、8px（`SNAP_THRESHOLD_PX`、`px_per_ms` で換算）以内で
  最も近いものへ寄せる。
- 移動は開始端・終端の両方を評価し、吸着した端のうち調整量の
  小さい方を採用する（`Drag::snapped_command_for`）。
- テスト `snap_picks_nearest_clip_edge_within_threshold`（イシュー
  指定）・`snap_targets_include_playhead_zero_and_grid`・
  `snap_excludes_the_dragged_clips_own_edges` が担保。
- 実アプリ証拠: opening（0..2000）の本体をドラッグし、終端候補
  2452ms が interview の開始 2500ms に吸着して開始が 500ms に
  なったことを状態ダンプで確認（無吸着なら 452ms）。

### AC2: トリムでも同じ吸着が効き、隣に接する位置で隙間なく止まる — ✅

- トリムは動かしている端だけを吸着させ、反対側の端を保つよう
  duration を再計算する。テスト `drag_trim_snaps_edges` が担保。
- 実アプリ証拠: opening の右端をドラッグし、終端候補 2452ms が
  2500ms に吸着（duration 2452）したことをダンプで確認。

### AC3: Alt を押しながらドラッグすると吸着が無効になる — ✅

- `main.rs` の `alt_down`（LAlt/RAlt）が `snap_enabled` を切り、
  無効時は候補コマンドをそのまま返す。テスト
  `drag_move_snaps_end_to_neighbor_start` の Alt ケースが担保。
- 実アプリ証拠: Alt を押しながら同じ方向へドラッグし、開始が
  候補どおり 48ms（無吸着）になったことをダンプで確認。

## 実装中に見つけた不具合（その場で修正）

- **移動スナップで終端の吸着が捨てられる**: 初版は開始端・終端の
  調整量を比べていたが、開始端が吸着しない場合の調整量 0 が常に
  勝ち、終端だけが近いケースで吸着が発動しなかった。単体テスト
  （両端とも吸着するケース）では検出できず、実アプリのドライブで
  「候補どおり 452ms にしかならない」ことから発覚した。吸着した
  端だけを候補にする方式に直し、回帰テスト
  `drag_move_snaps_end_even_when_start_has_no_target` を追加した。

## TDD の記録

- `snap_picks_nearest_clip_edge_within_threshold` 他を先に書き、
  `error[E0425]: cannot find function snap_edge` で落ちることを
  確認してから実装した。
- 上記不具合の回帰テストは、実アプリ相当の構成（デモと同じ
  クリップ配置）で落ちることを確認してから修正した。

## コードレビュー

- 独立 code-reviewer（新規サブエージェント、イシュー番号と diff
  範囲 `9e18f66..1a92aa1` のみを提示）に依頼し、**ラウンド1で
  APPROVE**。受け入れ条件3件と証拠ダンプの機械検証まで確認された。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red の記録は上記「TDD の記録」）
- [x] `cargo test` が 0 で終わった（`./scripts/verify.sh` 緑、225 件）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断があれば ADR を同じ commit で足し、関係する文書とルールを
      直した（新規の設計判断なし）
- [x] 試験レポートと証拠を置いた（本ファイルと `evidence/issue-0045/`）
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
      （close コメントで実施）
