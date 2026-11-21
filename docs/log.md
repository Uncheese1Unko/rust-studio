# ドキュメントの更新履歴

## 2026-11-21
* **Addition**: 試験レポート issue-0014 / issue-0016 / issue-0018 を追加した。
* **Addition**: 試験レポート issue-0011 / issue-0015 を追加した。
* **Addition**: 試験レポート issue-0013 / issue-0019 / issue-0022 を追加した。
* **Addition**: ADR-0011（人物切り抜きは MODNet をワーカースレッドでオフライン処理する、PBI #22）を追加した。
* **Addition**: ADR-0010（GUI と MCP の状態共有はプロジェクトファイル経由とする、PBI #11）を追加した。
* **Update**: 調査レポート issue-0021（人物切り抜き方式、PBI #21）を追加した。実素材での比較の結果、#22 の実装には MODNet を推奨する。
* **Update**: 試験レポート issue-0010（プレビュー再生、PBI #10）を追加した。メディア層をアプリ側に新設し、再生ヘッドに同期したフレーム表示を実装した。
* **Update**: 試験レポート issue-0009（クリップ配置・移動・トリミング、PBI #9）を追加した。コアに TrimClip コマンドを追加し、MCP の trim_clip ツールとしても公開した。
* **Update**: 試験レポート issue-0008（タイムライン UI、PBI #8）を追加した。video-editor がコアの状態をそのまま描画し、再生ヘッドが移動する。
* **Update**: 試験レポート issue-0006（CI）を完了版に更新した。ユーザーがワークフロー（`2cc472c`）を追加し Actions が緑になったため、Issue #6 を close した。
* **Addition**: 試験レポート issue-0004（HTTP/JSON API）を追加した。
* **Addition**: `crates/studio-mcp`（MCP サーバー、PBI #5）を追加した。stdio の JSON-RPC でコマンド群を tool として公開する。
* **Addition**: 試験レポート issue-0005（MCP サーバー）を追加した。
* **Addition**: `scripts/verify.sh`（ローカル検証の一括実行、PBI #6）と試験レポート issue-0006（途中経過）を追加した。ワークフローファイルの追加は権限のため needs-human で引き継ぎ中。
* **Addition**: 調査レポート issue-0017（ピッチ方式選定、PBI #17）を追加した。聴き比べは未実施のため needs-human で引き継ぎ中。
* **Addition**: ADR-0009（ピッチシフト方式に Signalsmith Stretch を採用）を追加。調査レポート issue-0017 に測定と聴取結果を追記し、#17 を閉じた。
* **Addition**: ADR-0008（HTTP API は std のみの薄いラッパーとして実装する）を追加した。
* **Addition**: 試験レポート issue-0003（JSON 永続化）を追加した。
* **Addition**: 試験レポート issue-0002（nannou ビルドと最小ウィンドウ表示の検証）を追加した。
* **Addition**: `docs/harness/index.html`（ハーネス設計の HTML＋SVG 要約資料）を追加した。
* **Addition**: PBI を4件追加した。サンプラー楽器化（EPIC #12）、DAW の UI デザイン（参照動画待ち、needs-human）、人物切り抜きの方式調査と実装（EPIC #7）。
* **Addition**: ADR-0007（サンプリング文化フレンドリー：性質を保ったまま音程を変える）を追加。方式選定の調査 PBI と DAW 実装 PBI を起票した。
* **Creation**: リポジトリを作成。ハーネス（`.claude/rules/`、`.claude/agents/`）、
  ADR-0001〜0006、アーキテクチャ概要、Cargo ワークスペースと studio-core の
  初期実装（モデル＋コマンド層、テスト付き）を追加した。
