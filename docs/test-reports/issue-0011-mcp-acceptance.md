# 試験レポート： Issue #11 MCP 編集受け入れ（video-editor）

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ、二周目）
- 対象コミット: `fe17e43`（実装）, `fdf158c`（レビュー対応）
- CI: 緑（スタックをまとめて push し、先端 cb3ce07 で確認）: https://github.com/Uncheese1Unko/rust-studio/actions/runs/44874493880
- 環境: Yoko の VM（Linux, ヘッドレス）。表示確認は Xvfb＋Mesa lavapipe。
  実ディスプレイでの確認は未検証。

## 実装の概要

- GUI と MCP の状態共有はプロジェクトファイル経由（ADR-0010）:
  - MCP サーバーに `--project-file` モードを追加。起動時に読み込み、
    変更系ツールの成功たびに保存する。
  - video-editor に `STUDIO_PROJECT_FILE` モードを追加。起動時に
    読み込み、内容ハッシュの変化を監視して自動で読み直し、GUI の
    編集も同じファイルへ保存し直す（双方向）。
  - 自分の保存で再読込ループが起きないこと（保存バイト列と
    ダンプのスタンプ一致）を不変条件テストで固定。

## 受け入れ条件ごとの結果

### AC: MCP でクリップ追加・移動を行い、GUI の表示と状態ダンプが一致する — ✅

- 受け入れ実行（証拠: [evidence/issue-0011/accept.sh](evidence/issue-0011/accept.sh),
  [mcp-out.jsonl](evidence/issue-0011/mcp-out.jsonl),
  [gui-stdout.txt](evidence/issue-0011/gui-stdout.txt),
  [shot1-mcp-state.png](evidence/issue-0011/shot1-mcp-state.png),
  [shot2-after-move.png](evidence/issue-0011/shot2-after-move.png),
  [project.json](evidence/issue-0011/project.json)）:
  MCP でクリップ追加・移動 → GUI が自動で読み直し、スクリーン
  ショットと GUI の再読込ダンプが MCP の get_project と一致した。

## TDD の記録

- スタンプ不変条件のテスト（保存直後に監視が誤判定しないこと）を
  先に書き、実装で満たした。
- レビュー対応では、読み込み失敗時に元ファイルが失われないことの
  再現テストを先に追加し、落ちることを確認してから直した。

## ゲート出力

- `./scripts/verify.sh`: 各コミット時点で終了コード 0。

## コードレビュー

- ラウンド1: `CHANGES_REQUESTED`。指摘4件はいずれも実在を確認し、
  テスト先行で修正した（コミット `fdf158c`）:
  - [Major] MCP の起動時読み込みに失敗すると backing が有効なまま
    になり、後続の保存で元ファイルを上書きしてしまう
    → 読み込み失敗時は backing を無効化し、元ファイルを保持。
  - 保存失敗がツール結果に現れない → `persisted: false` を返す。
  - GUI の再読込で進行中のドラッグ状態が残る → 再読込で打ち切る。
  - コアの `save_to_path` が非アトミック → tmp＋rename に変更。
  - あわせて docs/log.md の ADR-0010 記録漏れを補った。
- ラウンド2: `APPROVE`。非ブロッキング観察:
  - アトミック保存の tmp 名が固定（`<path>.tmp`）のため、GUI と MCP の
    同時保存で衝突しうる（将来の改善点）。
  - アトミック性のテストは「tmp が残らない＋往復」を固定するもので、
    読み取り中の不可視性そのものは直接検証していない。
  - ドラッグ打ち切りには自動テストがない（nannou の App 依存）。
  - backing 無効時に `persisted` フィールドは false ではなく不在になる。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red を上記に記録）
- [x] `cargo test` が 0 で終わった（飛ばし・弱体化なし）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した（2ラウンド）
- [x] この変更の commit で CI が緑（スタック先端 cb3ce07 で確認）
- [x] 判断を ADR-0010 に記録した（決定者: エージェント）
- [x] 試験レポートと証拠を置いた
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
      （実施済み）
