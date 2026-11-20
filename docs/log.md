# ドキュメントの更新履歴

## 2026-11-21
* **Addition**: 試験レポート issue-0004（HTTP/JSON API）を追加した。
* **Addition**: `crates/studio-mcp`（MCP サーバー、PBI #5）を追加した。stdio の JSON-RPC でコマンド群を tool として公開する。
* **Addition**: 試験レポート issue-0005（MCP サーバー）を追加した。
* **Addition**: `scripts/verify.sh`（ローカル検証の一括実行、PBI #6）と試験レポート issue-0006（途中経過）を追加した。ワークフローファイルの追加は権限のため needs-human で引き継ぎ中。
* **Addition**: ADR-0008（HTTP API は std のみの薄いラッパーとして実装する）を追加した。
* **Addition**: 試験レポート issue-0003（JSON 永続化）を追加した。
* **Addition**: 試験レポート issue-0002（nannou ビルドと最小ウィンドウ表示の検証）を追加した。
* **Addition**: `docs/harness/index.html`（ハーネス設計の HTML＋SVG 要約資料）を追加した。
* **Addition**: PBI を4件追加した。サンプラー楽器化（EPIC #12）、DAW の UI デザイン（参照動画待ち、needs-human）、人物切り抜きの方式調査と実装（EPIC #7）。
* **Addition**: ADR-0007（サンプリング文化フレンドリー：性質を保ったまま音程を変える）を追加。方式選定の調査 PBI と DAW 実装 PBI を起票した。
* **Creation**: リポジトリを作成。ハーネス（`.claude/rules/`、`.claude/agents/`）、
  ADR-0001〜0006、アーキテクチャ概要、Cargo ワークスペースと studio-core の
  初期実装（モデル＋コマンド層、テスト付き）を追加した。
