# 試験レポート： Issue #62 [DAW] 複製・貼り付けで offset_ms（分割位置）を引き継げるようにする

- 日付: 2026-11-22
- 実行者: エージェント（フォローアップ実施、監督: ヨーコ）
- 対象: #33 の複製・貼り付けで、分割したクリップの `offset_ms` が
  コピー側に引き継がれず、素材の先頭から再生されてしまう制限の解消。
- 実装コミット: `8adf58f`
- CI: 緑 https://github.com/Uncheese1Unko/rust-studio/actions/runs/45020278484
- 環境: Yoko の VM（Linux、ヘッドレス）。表示確認は Xvfb（:95）＋Mesa
  lavapipe（ソフトウェアレンダリング）。**実ディスプレイでの確認は未検証。**
- テスト件数: ベースライン 273 件 → 278 件（新規5件: core 2件、
  MCP 2件〈一覧の期待値追加＋tools/call 適用テスト〉、DAW 2件
  〈edit 1件・audio 1件〉。うち MCP はテスト関数としては新規1件）。
  削除・弱体化なし。

## 方式の判断（実装レベルの小判断）

イシューが挙げた2候補のうち、**`SetClipOffset` コマンドの新設**を採った。

- 既存の SetClipGain / SetClipPitch / SetClipFade 等と同じ「AddClip＋
  パラメータ設定コマンド」方式（#33/#44 の ClipPlacement の前提）と
  一貫する。`follow_up_commands` へ既定値（0）以外の場合だけ追加する
  形も既存パラメータと同じ。
- AddClip への offset 追加は、AddClip の JSON 形式（MCP / API /
  プロジェクト外の全呼び出し側が使う）を変えてしまうため採らない。
- 検証は `offset_ms >= 0` のみ。素材の長さはコアでは分からない
  （source は参照のみ）ため上限は検証せず、範囲外は再生側で
  無音／フレームなしに倒れる（render_mix の既存挙動）。

## 受け入れ条件ごとの結果

### AC1: 分割したクリップ（offset_ms > 0）を Ctrl+D / Ctrl+V で複製しても、コピー側の offset_ms が元と一致する — ✅

- コア: `Command::SetClipOffset { clip_id, offset_ms }` を新設
  （`crates/studio-core/src/command.rs`）。負値は InvalidRange で
  状態不変、未知のクリップは ClipNotFound。JSON 往復
  （type タグ `set_clip_offset`）もテストで担保。
- MCP: ツール `set_clip_offset` を公開（`crates/studio-mcp`）。
  tools/list の登録だけでなく tools/call 経由の適用・不正値の
  拒否までテストした（従来のレビュー観察への対応と同じ形）。
- DAW: `ClipPlacement` に `offset_ms` を保持し、0 でない場合に
  追随コマンドとして SetClipOffset を出すよう変更
  （`apps/daw/src/edit.rs`）。テスト
  `duplicate_command_copies_clip_offset`: start 1000 / duration 2000 の
  クリップを 2000 で分割して offset 1000 の右クリップを作り、
  複製（配置＋追随コマンド適用）で offset が 1000 に一致すること、
  貼り付けでも追随コマンドに SetClipOffset が含まれることを確認。
- 実アプリ証拠: プロジェクトファイル共有モードで offset_ms 1500 /
  gain 0.5 / pitch +2 のクリップを開き、実アプリで Ctrl+D →
  ダンプで 8500ms 位置の複製に offset_ms 1500 を確認。Ctrl+C →
  Ctrl+V でも貼り付け側（id=4）の offset_ms 1500 を確認
  （`evidence/issue-0062/gui-stdout.txt`）。ピクセル検証: 固定座標
  x=900 が初期状態の地色 (255,253,249) → 貼り付け後はクリップ面
  (255,199,222) に変化（表示範囲はプロジェクト長に追従するため、
  shot1 vs shot3 の固定座標で比較した）。

### AC2: render_mix の等価性テスト（分割クリップの複製前後で音が変わらない） — ✅

- テスト `render_mix_matches_after_duplicating_split_clip`
  （`apps/daw/src/audio.rs`）: 2000ms のクリップを 1000 で分割し、
  DAW の配置計画で右クリップを複製。元の右クリップだけのミックス
  （1000〜2000ms）とコピーだけのミックス（2000〜3000ms）が
  サンプル単位で一致（最大差 < 1e-6）することを確認。複製側の
  offset_ms が 1000 であることも assert している。

## TDD の記録

先にテストを書き、以下の red を確認してから実装した。

- core `command_tests`: `error[E0599]: no variant named SetClipOffset
  found for enum studio_core::Command`（コマンド未存在による
  コンパイルエラー）
- MCP `tools_list_exposes_all_commands`: `tool set_clip_offset がない`
  で panic。`set_clip_offset_tool_applies_via_tools_call`:
  tools/call が isError になり panic（mcp_tests.rs:915）
- DAW `duplicate_command_copies_clip_offset`: 同じく E0599
  （SetClipOffset 未存在）

実装後に全テストが通過し、`./scripts/verify.sh`（fmt / clippy /
workspace 全テスト）も緑。

## GUI ドライブの注記（正直な記録）

- 起動直後のアプリはフォーカスを得るまで描画も時計も進まない
  （elements: の出力だけでは不十分）。描画開始をスクリーン
  ショットの非黒ピクセルで確認してからキーを送る方式にした。
- 複製の適用後は、ポインタ状態（ドラッグ等）が残っていると
  後続の編集キーが `handle_edit_keys` の早期 return で捨てられる
  現象を実測した（Ctrl+C が複数回リトライしても発火しない）。
  中立位置（480,400）のクリック＋Escape で状態を落とすと発火する
  ようになったため、ドライブ手順に組み込んだ。アプリ側の状態
  残留そのものは本イシューの範囲外（受け入れ条件は offset の
  引き継ぎ）として、ドライブ側の回避に留めた。
- Xvfb 上では単発のキー送信が取りこぼされることがあり、各ステップは
  stdout のマーカーが出るまで再フォーカスしながら最大5回リトライ
  する方式（#31/#33 の方式）。

## コードレビュー

- 独立 code-reviewer（イシュー番号と diff 範囲 `a13c94f..8adf58f`
  のみを渡した）がラウンド1で **APPROVE**。指摘なし。
- レビュアーは受け入れ条件2件をコードで直接確認したほか、
  `follow_up_commands` の SetClipOffset を一時的に無効化すると
  `duplicate_command_copies_clip_offset` が落ちること
  （ミューテーションチェック）まで確かめた。
- 動画編集側 edit.rs に残る同様の制限コメントは #63 の範囲として
  切り分けられていることも確認された。
