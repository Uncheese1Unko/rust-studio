# 試験レポート: #95 MCP／HTTP API からエフェクトを操作し、効果を測定で確認できる（受け入れ確認）

対象 PBI: #95（EPIC #91「DAW トラックエフェクト」の4件目＝受け入れ確認）
実施日: 2026-11-25
対象コミット: なし（プロダクションコードの変更なし。#92 で追加した MCP ツール定義と、既存の汎用ディスパッチ（MCP／HTTP API とも）で受け入れ条件を満たすことを駆動で確認した）

## 駆動内容と結果（evidence/issue-0095/drive.sh、DRIVE OK）

### MCP（stdio JSON-RPC、--project-file 付き）

`mcp_fx.py` の確認結果:

- `add_track_effect`（delay 200ms / feedback 0.5 / mix 0.5）が成功し、`get_project` のダンプに `{"effect": {"type": "delay", "time_ms": 200, "feedback": 0.5, "mix": 0.5}, "bypassed": false}` が現れる。
- `set_track_effect_bypass` の往復（true → false）がダンプに反映される。
- 不正なコマンド2件（eq3 の low_gain_db 99.0、remove の index 9）はどちらも `isError` で、実行前後のダンプ文字列が完全一致（状態不変）。
- サーバー終了後、プロジェクトファイルに最終状態が永続化されている。

### 音の測定（MCP で作った状態を daw --export-wav で書き出し）

インパルスクリップの書き出しを測定: dry 0.8000、echo@200ms **0.4000**、@400ms **0.2000**、@600ms **0.1000**、@100ms は 0。ドライ比で **0.5 / 0.25 / 0.125**（設定どおり、許容 ±0.03 以内）。MCP 経由で組んだチェーンが再生・書き出しの音に効くことを、コア単体（#93）とは別経路で再確認した。

### HTTP API（studio-api、POST /commands + GET /project）

`api_fx.py` の確認結果:

- `add_track` → `add_track_effect`（eq3 low +6.0dB）が 200 で、GET /project に同じ値が現れる。
- `set_track_effect_bypass` が 200 で反映される。
- 不正なコマンド（delay の feedback 2.0）は **400** で、実行前後の GET /project が完全一致（状態不変）。
- `POST /undo` でバイパスが巻き戻る（API 経由でも Undo が効く）。

## テスト件数

- 変更なし（ワークスペース **504 件** のまま、全て通過）。本 PBI は新規コードではなく両経路の受け入れ確認であり、駆動スクリプトとログを証拠として残す。

## 独立レビュー

- （実施後に記録する）

## Definition of Done チェック

- [x] 受け入れ条件をすべて満たす（MCP の追加・バイパス・ダンプ反映、書き出しでの効果測定、両経路の不正コマンド拒否と状態不変、API の Undo）
- [x] 駆動証拠を `evidence/issue-0095/` に保存（drive.sh / mcp_fx.py / api_fx.py / drive-log.txt）
- [x] `./scripts/verify.sh` 緑（#94 完了時点の 504 件を維持）、CI 緑
- [x] 新規の設計判断なし
