# rust-studio

AviUtl 風の動画編集ソフトと Reaper 風の DAW を、Rust + nannou で作るモノレポ。
どちらも AI ネイティブ：編集操作は共通コアのコマンド層に集約され、GUI・HTTP API・
MCP サーバーの全経路がそこを通る。アーキテクチャの入口は `docs/architecture/overview.md`。

## 作業の進め方

- 作業は **EPIC → PBI** のイシュー階層で管理する。PBI は振る舞いの縦スライスで、
  起票時に根本原因・受け入れ条件（確認方法付き）・触るファイル・依存関係まで書く。
  ルールとテンプレート： `.claude/rules/issue-authoring.md`
- **デリバリーループ**：1イテレーション = 1 PBI を着手から close まで。
  順序は EPIC の一覧どおり、blocked by が閉じるまで先に進まない。
  手順： `.claude/rules/delivery-loop.md`

## ドキュメントと判断

- 判断（アーキテクチャ・技術・横断ルール）は `docs/adr/` に ADR として、
  その変更と同じコミットで残す。採用済み ADR は編集せず supersede する。
- **設計判断は人間のもの**。AI が判断に当たったら `needs-decision` ラベルで止まり、
  選択肢と推奨を提示して人間に渡す（判断 0006）。実装レベルの小判断は AI が決めて
  ADR に記録し、最終報告に挙げる。
- ルール： `.claude/rules/documentation.md`

## アーキテクチャ

- 共通コア `crates/studio-core`：タイムライン／トラック／クリップのモデルと
  コマンド層。**状態の変更はコマンド経由のみ**（GUI も API も MCP も同じ経路）。
- アプリ2本（`apps/video-editor`、`apps/daw`）は nannou 製の GUI と
  メディア処理を持ち、モデルとコマンドはコアに委ねる。
- ルール： `.claude/rules/architecture.md`、判断： ADR-0002〜0005

## 品質ゲート

- 実装は**常時 TDD**（red を試験レポートに記録する）。ルール： `.claude/rules/tdd.md`
- push 前に**独立コードレビュー**：新規の `code-reviewer` サブエージェントに
  イシュー番号と diff 範囲だけを渡す。APPROVE まで回す。
  ルール： `.claude/rules/code-review.md`、定義： `.claude/agents/code-reviewer.md`
- 完了の定義は `.claude/rules/definition-of-done.md`。試験レポート（証拠付き）を
  `docs/test-reports/` に残してから close する。
- バグは見つけた時点で直し切る。ルール： `.claude/rules/bug-handling.md`

## ハーネスを現行に保つ

ルール（`.claude/rules/`）と ADR はこのプロジェクトのハーネスである。
判断・繰り返すミス・新しい制約が出たら、それを促した変更と同じコミットで
ルールと ADR を更新する。会話の中だけにあるルールは存在しないものとする。
2回起きたミスは、ルールの文章を増やすのではなくチェック（テスト／スクリプト）にする。
