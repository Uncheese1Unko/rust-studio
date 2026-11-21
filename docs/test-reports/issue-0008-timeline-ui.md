# 試験レポート： Issue #8 タイムライン UI（トラック／クリップ表示と再生ヘッド）

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ、二周目）
- 対象コミット: `af35c60`（タイムライン UI 実装）
- CI: https://github.com/Uncheese1Unko/rust-studio/actions/runs/44865114239 (success)
- 環境: Yoko の VM（Linux, ヘッドレス）。表示確認は Xvfb＋Mesa lavapipe
  （ソフトウェア Vulkan）。**実ディスプレイ／実 GPU での表示は未検証**
  （#2 の申し送りと同じ）。

## 実装の概要

- `apps/video-editor/src/timeline.rs`（新規）: GUI 非依存のレイアウト計算
  （時刻→x 変換、クリップ矩形、再生ヘッド位置、終端計算、巻き戻し）と
  UI 要素の識別子（ADR-0005）を純粋関数で提供する。
- `apps/video-editor/src/main.rs`: nannou アプリ。起動時にデモプロジェクトを
  **コマンド経由で**構築し（状態変更はコマンド層のみ、ADR-0004）、
  状態ダンプと要素識別子の一覧を標準出力に出す。毎フレーム Project を
  そのまま描画し、再生ヘッドは実時間で進み終端で巻き戻る。

## 受け入れ条件ごとの結果

### AC1: コアの状態（トラック／クリップ）がそのまま画面に描かれる — ✅

- 確認方法（イシュー指定）: スクリーンショットと状態ダンプの突き合わせ。
- アプリ標準出力のダンプ（証拠: [evidence/issue-0008/app-stdout-dump.txt](evidence/issue-0008/app-stdout-dump.txt)）:
  V1 トラックに opening（0ms, 2000ms）と interview（2500ms, 4000ms）、
  A1 トラックに bgm（0ms, 8000ms）。
- スクリーンショット（証拠: [evidence/issue-0008/timeline-t6s.png](evidence/issue-0008/timeline-t6s.png)）
  のピクセル解析: ダンプの start/duration からレイアウト式で求めた矩形領域の
  最頻色が、opening／interview は映像クリップ色 (64,115,217)、bgm は
  音声クリップ色 (64,178,102) と完全一致した。トラック名（V1／A1）と
  クリップ名も画面に表示されていることを目視で確認した。

### AC2: 再生ヘッドが時刻に沿って移動する — ✅

- 確認方法（イシュー指定）: 実アプリでの目視と証拠。
- 起動後約2秒の撮影（[evidence/issue-0008/timeline-t2s.png](evidence/issue-0008/timeline-t2s.png)）
  では再生ヘッド列 x≈300（約2149ms 相当）、約5.9秒の撮影
  （timeline-t6s.png）では x≈578（約5458ms 相当）。時刻の経過に沿って
  右へ移動している。撮影・起動のレイテンシにより壁時計との差は数百ms 級。
- 進行と巻き戻しのロジック自体は単体テストで固定した（下記 TDD の記録）。

### AC3: UI 要素に安定した識別子がある（ADR-0005） — ✅

- 確認方法（イシュー指定）: 要素名の記録。
- 起動時に標準出力へ一覧を出す。実測:
  `timeline, timeline.ruler, timeline.playhead, timeline.track.1, timeline.clip.3, timeline.clip.4, timeline.track.2, timeline.clip.5`
  （数字はコアの要素 id。ダンプの id と一致）。
- 一意性と安定性（同じ Project からは常に同じ一覧）は単体テストで固定した。

## TDD の記録

- `timeline.rs` のテスト6件を先に書き、全関数を `unimplemented!()` の
  スタブにした状態で実行し、6件すべてが "not implemented" で落ちる
  （red）ことを確認した。その後実装して green（6件通過）。
  - clip_rect_reflects_start_and_duration
  - layout_covers_every_clip_in_project
  - playhead_x_matches_time
  - advance_playhead_moves_and_wraps
  - project_end_is_max_clip_end
  - element_ids_are_unique_and_stable
- 実行中の前提崩れの記録: 初回起動時に `NoAvailableAdapter` でパニック
  （#2 と同症状）。原因は VM の環境リセットで Mesa lavapipe
  （mesa-vulkan-drivers）が消えていたこと。再導入して解消した。
  これは調査環境側の変更であり、リポジトリの変更ではない。

## ゲート出力

- `./scripts/verify.sh`: 終了コード 0（fmt / clippy -D warnings /
  cargo test --workspace、テスト合計30件通過 = 従来24件＋新規6件）。
- CI（コミット `af35c60`）: success（上記 URL）。

## コードレビュー

- ラウンド1: レビュアーが最終報告を返さずに終了したため、結果なしとして
  新しいレビュアーでやり直した。
- ラウンド2（新規レビュアー）: `APPROVE`。受け入れ条件3件すべてが
  コード・テスト・証拠で裏付けられ、状態変更のコマンド層経由
  （ADR-0004）も守られている、ブロッキング指摘なし。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red を上記に記録）
- [x] `cargo test` が 0 で終わった（30件通過、飛ばし・弱体化なし）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプ（アプリ標準出力）でも同じ状態が観測できる
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断なし（ADR なし。レイアウトを GUI 非依存モジュールに置く構成は
      既存ルールの範囲内の実装判断として本レポートに記録）
- [x] 試験レポートと証拠を置いて push し、CI が緑
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
