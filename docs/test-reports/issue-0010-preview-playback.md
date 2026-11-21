# 試験レポート： Issue #10 プレビュー再生（映像の最小経路）

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ、二周目）
- 対象コミット: `f546fe6`（プレビュー再生の実装）, `1feb522`（レビュー観察対応）
- CI: https://github.com/Uncheese1Unko/rust-studio/actions/runs/44867199168 (success)
- 環境: Yoko の VM（Linux, ヘッドレス）。表示確認は Xvfb＋Mesa lavapipe。
  実ディスプレイでの確認は未検証（#2 の申し送り）。

## 実装の概要（最小経路の設計）

- メディア処理はアプリ側 `apps/video-editor/src/media.rs` に閉じた
  （architecture ルール）。コアは素材への参照文字列だけを持ち続ける。
- 経路: 起動時に ffmpeg でデモ用サンプル素材（testsrc2 の映像2本＋
  サイン波の音声）を生成し、映像クリップを 10fps・320x180 の PNG 列に
  事前展開する。再生ヘッドの時刻から「覆っている映像クリップ」と
  「クリップ内ソース時刻」を純粋関数で求め、対応フレームのテクスチャを
  プレビュー領域（要素識別子 `preview`）に表示する。
- ffmpeg が無い環境では警告のみで、プレビュー無しでアプリは続行する。

## 受け入れ条件ごとの結果

### AC1: サンプル素材が再生ヘッドに同期して表示される — ✅

- 確認方法（イシュー指定）: 実アプリでの目視と証拠。
- 素材には testsrc2 のタイムスタンプが焼き込まれており、再生ヘッド位置と
  独立に照合できる。実測（証拠: [evidence/issue-0010/preview-opening.png](evidence/issue-0010/preview-opening.png),
  [evidence/issue-0010/preview-interview.png](evidence/issue-0010/preview-interview.png)）:
  - opening: 再生ヘッド 1524ms（スクリーンショットの赤線位置から算出）、
    プレビュー表示フレームの焼き込み時刻は 00:00:01.533。差 9ms。
  - interview: 再生ヘッド 5024ms → ソース時刻は 5024−2500=2524ms が期待値、
    焼き込み時刻は 00:00:02.533。差 9ms。
  いずれも 10fps 展開の1フレーム区間（100ms）以内で、別クリップ・
  開始オフセット付きでも同期している。レビュアーも画像を直接確認し、
  オーバーレイ表示・ルーラー位置・焼き込み時刻の3指標の一致を認めた。
- 映像クリップの隙間（2000〜2500ms）ではプレビューが「no video clip」に
  なることも、標準出力の preview ログの途切れで確認できる
  （証拠: [evidence/issue-0010/app-stdout.txt](evidence/issue-0010/app-stdout.txt)）。

### AC2: メディア処理が studio-core に混入していない — ✅

- 確認方法（イシュー指定）: コアの依存関係の確認。
- `cargo tree -p studio-core` は serde / serde_json のみ
  （証拠: [evidence/issue-0010/studio-core-deps.txt](evidence/issue-0010/studio-core-deps.txt)）。
  本 PBI の diff は `crates/` を一切含まない。ffmpeg 呼び出しとフレーム
  展開は media.rs（アプリ側）のみ。

## TDD の記録

- media.rs の純粋関数4テスト（active_video_clip の境界、source_time_ms、
  frame_index、frame_path のクランプ）を先に書き、関数を
  `unimplemented!()` にした状態で4件すべてが落ちる（red）ことを確認
  してから実装した。
- レビュー観察対応では、frames_dir_name と clear_frame_files のテストを
  先に追加し、`cannot find function` のコンパイルエラー（red）を
  確認してから実装した。

## ゲート出力

- `./scripts/verify.sh`: 終了コード 0（テスト合計48件通過 =
  従来42件＋media 4件＋hygiene 2件）。
- CI（コミット `1feb522` まで）: success（上記 URL）。

## コードレビュー

- ラウンド1: `APPROVE`。非ブロッキング観察3件:
  (1) プレビュー観測ログが同一フレームでも重複発火する、
  (2) フレーム再展開時に古い PNG が残留する潜在バグ、
  (3) フレーム保存先が stem のみで同名素材が衝突する＋テクスチャ列挙の
  100ms ハードコード。
  (2)(3) は潜在バグとして bug-handling ルールに従いその場で直し、
  (1) も合わせて対応した（コミット `1feb522`、テスト先行）。
- ラウンド2（新規レビュアー、対応コミットのみ審査）: `APPROVE`。
  残りの観察（キー計算の関数化、clear の述語の厳密化、FPS が 1000 を
  割り切らない場合の列挙）は現行 10fps では正確であり、将来の改善点
  として記録するに留める。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red を上記に記録）
- [x] `cargo test` が 0 で終わった（48件通過、飛ばし・弱体化なし）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプ（アプリ標準出力の preview ログ）でも同じ状態が観測できる
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した（2ラウンド）
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断なし（ADR なし。最小経路の選択は既存ルールの範囲内の実装判断
      として本レポートに記録）
- [x] 試験レポートと証拠を置いて push し、CI が緑
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
