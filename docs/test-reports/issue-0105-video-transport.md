# 試験レポート: #105 動画編集で再生／一時停止・シーク・先頭／末尾ジャンプができる

対象 PBI: #105「動画編集で再生／一時停止・シーク・先頭／末尾ジャンプができる」（EPIC #103）
実施日: 2026-11-27
ベース: `2b1c94a`（#104 のコミット）

根本原因は、GUI に再生状態が存在せず `update()` が毎フレーム無条件に再生ヘッドを進めていたこと。再生状態を GUI の操作状態として持ち、前進をトランスポート経由に変え、ルーラーとキー／ボタンからシークできるようにした。DAW 側 #104 と同じ形（両アプリの timeline.rs は従来から同型の複製）。

## 実装内容

- `apps/video-editor/src/timeline.rs`: 純粋関数として追加。
  - `PlayState`（`Playing` / `Paused`、`toggle()`、`is_playing()`）。
  - `transport_advance(state, current_ms, dt_ms, end_ms)`: 停止中は現在位置をそのまま返し、再生中は既存の `advance_playhead` に委ねる（終端ラップの既存挙動を維持）。
  - `seek_clamp(ms, end_ms)`: `[0, 終端]` にクランプ。終端 0（空プロジェクト）なら常に 0。
- `apps/video-editor/src/main.rs`:
  - Model に `play_state`（初期値 `Paused` ＝起動時は停止）、スペース／Home／End の押下エッジ追跡、スクラブ中フラグ `seek_drag` を持つ。キー処理はテキスト編集中でないときだけ行い、編集中は prev を同期だけする（バグ #60 のパターン。対象セッションはリネーム、タイトル入力、スクリプト名／パラメータ入力）。
  - `update()` の再生ヘッド前進を `transport_advance` 経由にした。プレビュー、トランジションのブレンド、スクリプトフレームの更新はすべて再生ヘッド連動のため、停止／シークにそのまま追従する。
  - シークは `seek_to` ヘルパーに集約: クランプして `playhead_ms` を設定し、追従が有効なら `ViewScale::follow_playhead`（巻き戻り判定は「新位置 < 旧位置」）で表示を合わせ、`seek: <ms>` を stdout に出す。トグル時は `transport: playing` / `transport: paused` を出す（観測経路、ADR-0005）。
  - ルーラー（y < RULER_HEIGHT_PX かつ x >= HEADER_WIDTH_PX）: 左押下で即座にシークして `seek_drag` を立て、保持中は位置が変わるフレームだけシークし直す（スクラブ）。解放で下ろす。スクラブ開始フレームは早期 return し、クリップドラッグ等は始まらない。
  - トランスポートボタンはルーラーのヘッダー列（x=0..120, y=0..24）を3分割: 先頭 START（x=0..40）、再生／一時停止 PLAY/PAUSE（x=40..80、再生中は面のグレー濃度 0.22→0.38＋ PAUSE ラベル）、末尾 END（x=80..120）。ラベルは font_size 9。

## 状態ダンプに関する判断

再生状態と再生ヘッド位置はプロジェクトの保存対象ではない（表示・操作の状態）。状態ダンプ（プロジェクトダンプ）には含めない。根拠と先例は #104 のレポートと同じ（メトロノーム #36、ループ範囲 #40、再生追従 #84/#86 はいずれもプロジェクトに保存せずダンプにも含めない GUI 操作状態）。トランスポートの観測は、画面（ボタンの見た目とプレビュー）と stdout ログ（`transport:` / `seek:` / `scroll offset:` / `preview:`）で行う。

## TDD の記録（red）

単体テスト4件（`play_state_toggles`、`transport_advance_holds_when_paused`、`transport_advance_moves_and_wraps_when_playing`、`seek_clamp_bounds`）を先に書き、`cargo test -p video-editor --bin video-editor timeline` で `PlayState` / `transport_advance` が未定義（E0433 / E0425、計25件）のコンパイル失敗を確認した。実装後に green。

## 単体テスト

- ワークスペース全体: 534 件 → 538 件（新規4件は `apps/video-editor/src/timeline.rs`）、全て通過。
- `./scripts/verify.sh`: fmt / clippy / テストすべて OK。

## 実アプリ駆動（GUI 証拠）

Xvfb（display :99）＋ lavapipe ソフトウェアレンダリングで実アプリを駆動した。**実ディスプレイ／実 GPU での見え方は未検証。**

駆動スクリプトは `evidence/issue-0105/drive.sh`、標準出力ログは `evidence/issue-0105/gui-stdout.txt`、スクリーンショットは同ディレクトリの `01`〜`11`。デモプロジェクト（3トラック、終端 8,000ms）で全21チェック PASS:

- 起動直後: 2.5秒待っても画面が完全一致のまま（停止状態でヘッドが動かない）。`preview:` ログも初期の1件から増えない（プレビューが進まない）。
- スペース長押し → `transport: playing` が出てヘッドとプレビューが進む（`preview:` ログが増え、画面が変化）。再度押す → `transport: paused` が出て、その位置で画面が2秒間完全一致のまま止まる。
- ルーラークリック（x=540）→ `seek: 5000` が出てヘッドがその位置へ移動。
- ルーラードラッグ（x=300→420→540）→ `seek: 2143 / 3571 / 5000` と連続し、なぞった位置へ追従する（スクラブ）。
- Home → `seek: 0`、End → `seek: 8000`。
- ボタン: END → `seek: 8000`、START → `seek: 0`、PLAY → `transport: playing`、PAUSE → `transport: paused`。再生中の中央ボタンは PAUSE 表示で面が明るくなる（`09-playing-by-button.png`）。
- タイトル編集中（V1 を選択しクリップ内 1,429ms へシークして T で開始）にスペースを押しても `transport:` ログは増えない。
- Ctrl+ホイールでズームして表示範囲を狭めた状態で、停止中に End → `seek: 8000` と同時に `scroll offset: 6817` へ変わり、追従表示がシーク先へ追いつく。
- 駆動上の注意: #104 と同じく、xdotool の瞬時 `click` はポーリングで取りこぼすため、押下を 0.35 秒保持する方式を使った（アプリ側の問題ではない）。

## 独立レビュー

- ラウンド1: APPROVE（指摘なし）。受け入れ条件7件すべてのコード上の裏付け、スコープ（触るファイルは指定の2件のみ、DAW 無変更）、状態ダンプにトランスポート状態が含まれないこと、テストが純粋に加算のみであることが確認された。

## Definition of Done チェック

- [x] 受け入れ条件をすべて、書いてある確認方法で確かめた
- [x] TDD（red を記録）
- [x] `./scripts/verify.sh` 緑（テスト 538 件全通過）
- [x] 独立レビュー APPROVE（ラウンド1）
- [x] 実アプリ駆動で証拠を取得（Xvfb + lavapipe、実ディスプレイ未検証と明記）
- [x] 試験レポートと証拠を配置
- [x] CI 緑（push 後に確認し、クローズコメントに記録）
