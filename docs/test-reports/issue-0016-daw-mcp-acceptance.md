# 試験レポート： Issue #16 DAW の MCP 編集受け入れ

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ、二周目）
- 対象コミット: `1ab566b`（実装）, `e36e6fe`（レビュー対応: 再読込時の波形保持）
- CI: スタック push 後に記録する
- 環境: Yoko の VM（Linux, ヘッドレス）。表示確認は Xvfb＋Mesa lavapipe。
  実ディスプレイでの確認は未検証。

## 実装の概要

- video-editor の #11 と同じ方式（ADR-0010）を DAW に適用:
  環境変数 `STUDIO_PROJECT_FILE` でプロジェクトファイルを共有し、
  起動時に読み込み、内容ハッシュの変化を監視して自動で読み直す。
  GUI の編集（ドラッグ・配置・ミキサー）は同じファイルへ保存し直す。
- 再読込時は進行中のドラッグ／ミキサー操作を打ち切り、ソース集合が
  変わった場合のみ波形を再計算する。

## 受け入れ条件ごとの結果

### AC: MCP でクリップ追加・ゲイン変更を行い、GUI と状態ダンプが一致する — ✅

- 受け入れ実行（証拠: [evidence/issue-0016/accept.sh](evidence/issue-0016/accept.sh),
  [mcp-out.jsonl](evidence/issue-0016/mcp-out.jsonl),
  [gui-stdout.txt](evidence/issue-0016/gui-stdout.txt),
  [shot1-mcp-state.png](evidence/issue-0016/shot1-mcp-state.png),
  [shot2-after-mcp.png](evidence/issue-0016/shot2-after-mcp.png)）:
  - MCP で add_track / add_clip（tone-220, 0ms, gain 1.0）→
    GUI がその状態で起動し、クリップが描画された。
  - MCP で move_clip（→1000ms）と set_clip_gain（→0.5）→
    GUI が自動で読み直し、GUI のダンプが MCP の get_project と
    完全一致（start 1000, gain 0.5）。スクリーンショットでも
    クリップが新しい位置に描かれ、旧位置は空になった。

## TDD の記録

- 自己保存で再読込ループが起きないこと（スタンプ不変条件）のテストを
  先に書き、実装で満たした。
- レビュー対応では、ソース比較と波形保持のテスト
  （project_sources / sources_changed / refresh_waveforms）を先に
  書き、関数未実装のコンパイルエラー（red）を確認してから実装した。

## ゲート出力

- `./scripts/verify.sh`: 実装時点で終了コード 0（81 件通過）。
  レビュー対応後も daw のテスト全件通過（37 件）。

## コードレビュー

- ラウンド1: `CHANGES_REQUESTED`。受け入れ条件の証拠自体は成立し、
  ADR-0010 との整合・操作の打ち切り・ループ防止も確認された。
  - [Medium] 再読込のたびに配置用素材（dropped）の波形が失われる:
    `old_sources` を波形マップのキー（配置用素材を含む）から作り、
    プロジェクトのソース集合と比較していたため、ほぼ常に不一致と
    なり全再計算され、配置用素材の波形が消えていた。最初の外部
    再読込以降、ダブルクリック配置したクリップに波形が出なくなる
    回帰。→ 比較をプロジェクトのクリップソース同士で行い、
    再計算時は配置用素材の波形を保持するよう修正し、単体テストで
    固定した（コミット `e36e6fe`）。
- ラウンド2: 再審査で確認する。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red を上記に記録）
- [x] `cargo test` が 0 で終わった（飛ばし・弱体化なし）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [ ] push の前に独立した `code-reviewer` が `APPROVE` を返した
      （ラウンド2の結果を記録する）
- [ ] この変更の commit で CI が緑（push 後に記録する）
- [x] 判断なし（ADR-0010 の方式を DAW に適用したもの）
- [x] 試験レポートと証拠を置いた
- [ ] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
      （push 後に行う）
