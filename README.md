# rust-studio

Rust で作る、AI ネイティブな2つのマルチメディア制作アプリのモノレポ。

- **動画編集ソフト**（AviUtl 風）— `apps/video-editor`
- **DAW**（Reaper 風）— `apps/daw`

どちらも Rust のクリエイティブコーディング用フレームワーク **nannou**
（C++ における OpenFrameworks にあたるもの）で作る。

## AI ネイティブ設計

- 編集操作はすべて「コマンド」として `crates/studio-core` に定義され、
  GUI・API・MCP の全経路が同じコマンド層を通る（単一の変更経路）。
- **MCP サーバー**がコマンドを tools として公開する。「こうして」と指示すると
  アプリがそのとおりに動く。
- **HTTP/JSON API** がプロジェクト状態のダンプとコマンド実行を提供し、
  computer use（画面操作エージェント）によるテストを可能にする。
  UI 要素には安定した識別子を持たせ、状態は API で機械可読に返す。

## 開発方法（ハーネス）

ohta-rh 氏の x-modoki-v2 / insta-modoki の AI 駆動開発ハーネスに倣う。
入口は `CLAUDE.md`、本体は `.claude/rules/` と `docs/adr/`。

- 作業は EPIC → PBI のイシュー階層で管理し、デリバリーループで1件ずつ消化する
- 実装は常時 TDD、push 前に独立コードレビュー、完了には試験レポート（証拠付き）
- 判断は ADR に残す。**設計判断は人間のもの**：AI は `needs-decision` で止まる
  （ohta 方式の「聞かずに決める」は、設計判断については採用しない。判断0006）

## 構成

```
apps/video-editor/   動画編集ソフト（nannou）
apps/daw/            DAW（nannou）
crates/studio-core/  共通コア：タイムライン/トラック/クリップのモデルとコマンド層
docs/adr/            設計判断（ADR）
docs/architecture/   アーキテクチャ文書
docs/test-reports/   試験レポートと証拠
.claude/rules/       開発ルール（ハーネス本体）
.claude/agents/      サブエージェント定義
```

## ビルド・テスト

```sh
./scripts/verify.sh   # ローカル検証一括（fmt --check / clippy -D warnings / test）
cargo test            # コアのテスト
cargo run -p video-editor
cargo run -p daw
```

CI（GitHub Actions）でも同じ検証を push / PR のたびに回す
（`.github/workflows/ci.yml`）。
