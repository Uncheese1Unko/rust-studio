# 試験レポート： Issue #50 動画編集でクリップにテキスト（タイトル）を重ねられる

- 日付: 2026-11-22
- 実行者: エージェント（デリバリーループ、並行レーンB＝動画編集 担当）
- 対象: クリップにタイトル文字列を持たせ（Clip.title_text /
  SetClipTitle）、再生区間はプレビューの画面下部中央に白・28pxで
  重ねて表示する。GUI では T キーで入力モードに入り Enter 確定・
  Esc 中止。MCP からも設定できる。
- 実装コミット: `9dbcb30`（rebase 前 `6074a93`）
- CI: 緑 https://github.com/Uncheese1Unko/rust-studio/actions/runs/45009338986
- 環境: Yoko の VM（Linux、ヘッドレス）。表示確認は Xvfb（:92）＋Mesa
  lavapipe（ソフトウェアレンダリング）。**実ディスプレイでの確認は未検証。**
- テスト件数: ワークスペース全体 243 件 → 246 件（新規3件:
  コアの `set_clip_title_round_trips_through_json`、MCP の
  `set_clip_title_tool_applies_via_tools_call`、GUI セッションの
  `title_session_enter_commits_and_esc_cancels`。ほかに複製テストへ
  タイトル引き継ぎの確認を追加）。削除・弱体化なし。

## 受け入れ条件ごとの結果

### AC1: SetClipTitle でタイトルを設定でき、ダンプと JSON で観測・永続化できる — ✅

- `Clip.title_text`（serde default は空文字＝タイトルなし、旧形式
  JSON は空文字で読める）と `Command::SetClipTitle` を追加。
- イシュー指定テスト `set_clip_title_round_trips_through_json` が
  設定→ダンプ→再読込の一致、旧形式の読み込み、空文字での消去、
  存在しないクリップでのエラー時不変を担保。

### AC2: タイトルを持つクリップの再生区間では、プレビューの画面下部中央に文字列が表示される — ✅

- 描画は `media::active_video_clip` のタイトルをプレビュー下部
  中央の帯に白・font_size 28 で重ねる（カスタマイズなし）。
- 実アプリ証拠: 暗い単色素材のクリップに「Op3n」を設定し、
  タイトル帯の白ピクセル数が 0 → 291 に増えたことと、
  スクリーンショットで下部中央の表示を確認
  （`evidence/issue-0050/03-titled.png`）。ドライブのコンソール
  出力は本レポートに転記（レビューでの任意指摘への対応）:
  `white pixels in title strip before: 0` /
  `white pixels in title strip after: 291` /
  `OK title visible in preview (0 -> 291 white px)`。

### AC3: GUI でクリップを選んでタイトルを入力・変更・消去できる — ✅

- 選択中トラックの再生ヘッド位置にあるクリップ（複製・コピペと
  同じ対象規則）を対象に、T キーで `edit::TitleSession` を開始。
  Enter で SetClipTitle を履歴経由で適用し保存、Esc で中止、
  空で確定すれば消去。入力はイシューのスコープどおり英数字と
  空白・一部記号のみ（日本語は取り込まない）。
- 実アプリ証拠: T → 打鍵「Op3n」→ Enter でダンプに
  `"title_text": "Op3n"` が出ること、Esc 中止では漏れないことを
  ログで確認。

### AC4: MCP からタイトルを設定でき、ダンプで観測できる — ✅

- ツール `set_clip_title` を tools/list に追加（ディスパッチは
  従来の汎用経路＝コマンド層経由のみ）。
- MCP テスト `set_clip_title_tool_applies_via_tools_call` が
  tools/call での適用・ダンプ観測・エラー時不変・消去を担保。

## 実装上の判断（小さな判断として記録）

- **タイトル入力の開始は T キーの離しエッジ**にした。押下エッジで
  始めると、同じ押下の文字イベント（'t'）がセッション開始後に
  届いてバッファへ混入しうるため（文字イベントは押下時に届き、
  その時点ではセッションが無いので捨てられる）。
- **分割（SplitClip）の右クリップと複製・貼り付けはタイトルを
  引き継ぐ**。分割はフェードアウト引き継ぎの前例、複製は #44 の
  「全パラメータ」規則に合わせた（複製はテストで担保）。
- TitleSession の単体テストは実装と同時に書いた（コアと MCP の
  red は先に記録済み。下記）。

## TDD の記録

- コア: `set_clip_title_round_trips_through_json` を先に書き、
  `no field title_text` / `no variant named SetClipTitle` で
  落ちることを確認してから実装した。旧形式 JSON の作り方は
  末尾フィールドのカンマ無しに対応できず一度失敗し、Value 経由の
  除去に直した（テスト側の不備で、実装の不具合ではない）。
- MCP: テストを先に書いた時点で、ディスパッチが汎用（serde の
  type タグ）だったため呼び出し自体は通り、tools/list への
  定義追加が残作業になった（その状態も確認した）。

## コードレビュー

- 独立 code-reviewer（新規サブエージェント、イシュー番号と diff
  範囲 `517e1d7..6074a93` のみを提示）に依頼し、**ラウンド1で
  APPROVE**。edit.rs への配置（RenameSession の前例）と分割・
  複製での引き継ぎも妥当と判断された。任意の注記として、証拠に
  ドライブのコンソール出力（白ピクセル数の assert）が含まれて
  いない点が挙がり、本レポートの AC2 に出力を転記した。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red の記録は上記「TDD の記録」）
- [x] `cargo test` が 0 で終わった（`./scripts/verify.sh` 緑、246 件）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断があれば ADR を同じ commit で足し、関係する文書とルールを
      直した（新規の設計判断なし。判断は上記に記録）
- [x] 試験レポートと証拠を置いた（本ファイルと `evidence/issue-0050/`）
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
      （close コメントで実施）
