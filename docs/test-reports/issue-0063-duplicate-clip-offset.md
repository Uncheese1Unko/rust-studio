# 試験レポート： Issue #63 動画編集の複製・貼り付けで offset_ms（分割位置）を引き継げるようにする

- 日付: 2026-11-22
- 実行者: エージェント（フォローアップ実施、監督: ヨーコ）
- 対象: #44 の複製・貼り付けで、分割したクリップの `offset_ms` が
  コピー側に引き継がれず、素材の先頭から再生されてしまう制限の解消。
  コア側の方式は #62 で確定した `SetClipOffset` コマンドに追随する。
- 実装コミット: `5a80070`、レビュー観察対応 `57825fa`
- CI: 緑 https://github.com/Uncheese1Unko/rust-studio/actions/runs/45021964803
  （#63・#65・#64 の累積ツリーに対する push の run）
- 環境: Yoko の VM（Linux、ヘッドレス）。表示確認は Xvfb（:93）＋Mesa
  lavapipe（ソフトウェアレンダリング）。**実ディスプレイでの確認は未検証。**
- テスト件数: ベースライン 278 件 → 279 件（新規1件:
  video-editor `duplicate_command_copies_clip_offset`）。
  削除・弱体化なし。

## 変更内容

- `apps/video-editor/src/edit.rs` の `ClipPlacement` に `offset_ms` を
  保持し（`from_clip` で元クリップから引き継ぐ）、`follow_up_commands`
  が 0 でない場合に `Command::SetClipOffset` を出すようにした
  （DAW #62 と同じ形、既存パラメータと同じ「既定値では出さない」作法）。
- 「offset は引き継げない」という旧来のコメント類を実態に合わせて更新し、
  既存テスト `duplicate_command_copies_all_clip_parameters` にも
  offset の assert を追加した。

## 受け入れ条件ごとの結果

### AC1: 分割したクリップ（offset_ms > 0）を Ctrl+D / Ctrl+V で複製・貼り付けしても、コピー側の offset_ms が元と一致する — ✅

- 単体テスト `duplicate_command_copies_clip_offset`（イシュー指定名）:
  start 1000 / duration 2000 のクリップを 2000 で分割して offset 1000 の
  右クリップを作り、複製（配置計画＋追随コマンド適用）で offset が
  1000 に一致すること、貼り付けでも追随コマンドに SetClipOffset が
  含まれ、実際に適用した最終状態でも offset が一致することを確認。
- 実アプリ証拠: プロジェクトファイル共有モードで offset_ms 1500 の
  映像クリップ（0〜4000ms）を開き、実アプリで Ctrl+D →
  ダンプで 4000ms 位置の複製に offset_ms 1500 を確認。Ctrl+C →
  Ctrl+V でも貼り付け側（id=4）の offset_ms 1500 を確認
  （`evidence/issue-0063/gui-stdout.txt`）。

### AC2: 複製した右クリップのプレビューが分割前の続きの映像を表示する — ✅

- 実アプリのプレビューログで確認: 再生ヘッドが複製区間に入った直後の行が
  `preview: source=demo/opening.mp4 playhead=4017 source_time=1517 frame=15`
  で、複製の素材時刻が 1500 から続いている（offset が落ちていれば
  source_time は 17 になるはず）。
  スクリーンショット: `evidence/issue-0063/shot1-initial.png`（初期）、
  `shot2-duplicated.png`（複製後）、`shot3-pasted.png`（貼り付け後）。

## TDD の記録

先にテストを書き、以下の red を確認してから実装した。

- `duplicate_command_copies_clip_offset`:
  `assertion left == right failed: duplicate must keep the offset —
  left: 0, right: 1000`（追随コマンドが無く offset が 0 のまま）

実装後に当該テストが通過し、`./scripts/verify.sh`（fmt / clippy /
workspace 全テスト）も緑。

## GUI ドライブの注記（正直な記録）

- ドライブ初回、状態リセット用に送った Escape でアプリが終了した。
  原因は nannou 既定の「Esc で終了」が動画編集側で有効なままだった
  ことで、これは #63 の範囲外のバグとして **#65** を起票し、
  バグ作法どおり本件の完了前に修正した（試験レポート
  issue-0065 参照）。ドライブは Escape を送らない形に変更して再実行し、
  全項目を確認した。
- キーの送信方式（マーカーが出るまで再フォーカスしながら最大5回
  リトライ）は #62 と同じ。

## コードレビュー

- 独立 code-reviewer（イシュー番号と diff 範囲 `0f68f43..5a80070`
  のみを渡した）がラウンド1で **APPROVE**。
- レビュアーはミューテーションチェックを実施: SetClipOffset の
  push を消す／`from_clip` の offset を 0 に変異させる、いずれでも
  当該テストが落ちることを確認した。
- 非ブロッキング観察1件: 「貼り付け側のテストが追随コマンドの
  有無のみで、適用後の最終状態を見ていない」。共有経路は複製側で
  end-to-end に担保されているため必須ではないとされたが、
  状態で見る方が強いため、貼り付けを実際に適用して offset と
  start を assert する形にテストを強化した（コミット `57825fa`）。
