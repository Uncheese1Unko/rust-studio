# 試験レポート: #87 DAW のタイムライン見渡し手段

対象 PBI: #87「DAW でもタイムラインを素早く見渡せる（スクロールバーとドラッグパン、画面幅に比例したホイールスクロール）」
実施日: 2026-11-24
実装コミット: `461b283`
ベース: `14cafad`

動画側 #85（コミット `8d6919a`）の設計をそのまま流用した。DAW 固有の適応は、スクロールバーの色を DAW テーマ定数（帯＝LINE、つまみ＝TEXT_SUB、ドラッグ中＝ACCENT）にした点と、ホイールの無効化条件をリネーム／BPM 編集中とした点のみ。

## 実装内容

- `apps/daw/src/timeline.rs`: 純粋関数を追加。`wheel_delta_ms`（1ノッチ＝表示幅の 1/10、符号は y+＝戻る／y−＝進む／x+＝進む、x と y を合算）、`max_offset_ms`（上限＝終端 − 表示幅、下限 0）、`scroll_by_clamped`（スクロールの入口専用。`zoom_at` のアンカー保持には適用しない）、`scrollbar_thumb`（最小幅 20px、全表示時はトラック全幅）、`scrollbar_drag_to`（逆換算＋両端クランプ）、自由関数 `SCROLLBAR_HEIGHT_PX`（12px）／`scrollbar_y`（最終トラック行の直下）／`scrollbar_grab`（帯内のみ。つまみ内は実ずれ、つまみ外は中心掴み）。
- `apps/daw/src/main.rs`: `Model.scrollbar_drag`／`pan_drag` を追加（表示状態、保存しない）。`mouse_wheel` は横成分を捨てずに拾い（PixelDelta は両軸 50px＝1行）、Ctrl+ホイールは y 成分がある場合のみズーム、それ以外は `wheel_delta_ms`＋`scroll_by_clamped` でオフセットが変わったときだけ `scroll offset: N` を出力。`handle_pointer` ではピアノロール委譲の直後・他の左ボタン処理より前にスクロールバーの掴み／ドラッグ／解放（`scrollbar drag end: offset N`）を処理し、中ボタンドラッグでパン（掴んで動かす向き、終端クランプ）。描画はタイムライン下端の帯につまみを描く。

## TDD の記録（red）

単体テスト5件（`wheel_delta_is_proportional_to_visible_width`、`scroll_by_clamped_stops_at_project_end`、`scrollbar_thumb_reflects_offset_and_visible_ratio`、`scrollbar_drag_to_round_trips_and_clamps`、`scrollbar_grab_detects_band_and_thumb`）を先に書き、未実装のため E0425（`scrollbar_grab`／`scrollbar_y` が無い）と E0599（メソッド5件が無い）でコンパイル失敗を確認した。実装後に green。

## 単体テスト

- `cargo test -p daw`: 143 件 → 148 件（新規5件）。
- ワークスペース全体: 375 件 → 380 件、全て通過。
- `./scripts/verify.sh`: fmt / clippy / テストすべて OK。

## 実アプリ駆動（GUI 証拠）

Xvfb（display :97）＋ lavapipe ソフトウェアレンダリングで実アプリを駆動した。**実ディスプレイ／実 GPU での見え方は未検証。**

駆動スクリプトは `evidence/issue-0087/drive.sh`、標準出力ログは `gui-stdout.txt`、スクリーンショットは `01-initial.png`〜`03-after-checks.png`。プロジェクト終端は 21,000ms（表示幅 10,000ms、上限オフセット 11,000ms）。再生追従（#86）は F キーで切ってから計測した。

- ホイール: 1ノッチでちょうど 1,000ms（表示幅の 1/10）移動。下2クリック（＝4ノッチ）で 4,000、上1クリックで 2,000 に戻る。
- 横ホイール: ボタン6（x+）で進み、ボタン7（x−）で戻る。
- Shift+ホイール下: +2ノッチ分進む（x／y どちらの表現でも効く合算の確認）。
- 中ボタンパン: 左へ 150px×2ステップで各約 +1,786ms（150px ÷ 0.084px/ms）、合計 7,572 まで移動。
- スクロールバー: 初期表示でつまみが左側（ピクセル確認: x=200 はつまみ色、x=900 は帯色）。帯内のつまみの外を押すと中心掴みでオフセット 0 へ寄り、右端まで引くと `scrollbar drag end: offset 11000`（上限クランプ）。ドラッグ後は x=900 がつまみ色、x=200 が帯色に反転。
- 回帰: Ctrl+ホイールで `zoom:` ログ、クリップドラッグで `after drag:` ログを確認。
- 環境の知見: 本 X11 スタックでは xdotool の click 1回でホイールイベントが2ノッチ届く（press／release 両方）。期待値はこの前提で計算した（#85 の駆動でも同じ前提だったはずだが、当時は wait_log 方式のため表面化しなかった）。

## 独立レビュー

- ラウンド1: APPROVE。全受け入れ条件、クランプの適用範囲（入口のみ、`zoom_at` には不適用）、ピアノロール表示中は処理が働かないこと、スコープ外混入なし、ゲート弱体化なしが確認された。非ブロッキング注記として「diff 内に駆動証拠と試験レポートが無い」点が挙がったが、証拠は本レポートと `evidence/issue-0087/` で別途配置済みである。

## Definition of Done チェック

- [x] 受け入れ条件をすべて満たす（比例ホイール、横／Shift+ホイール、スクロールバー描画と割合、ドラッグ＋終端クランプ、中ボタンパン、既存操作の回帰なし）
- [x] TDD（red を記録）
- [x] `./scripts/verify.sh` 緑
- [x] 独立レビュー APPROVE（1ラウンド）
- [x] 実アプリ駆動で証拠を取得（Xvfb + lavapipe、実ディスプレイ未検証と明記）
- [x] 試験レポートと証拠を配置
- [x] CI 緑（push 後に確認）
