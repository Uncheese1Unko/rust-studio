# 試験レポート: Issue #72 Vital の音色でメロディーを鳴らせる（CLAP ヘッドレス連携）

## 受け入れ条件と結果

具体化した受け入れ条件（イシューコメントで合意した4件）で検証した。

| # | 条件 | 結果 |
|---|------|------|
| 1 | Plugin バリアント（CLAP＋プリセット参照）をコマンド／MCP で設定でき、ダンプで観測できる | ✅ コアテスト（dump・往復）＋ MCP テスト（set_track_instrument 経由で適用とダンプ確認、空パスはエラーで状態不変） |
| 2 | Plugin トラックのノートが別プロセスの CLAP ホスト経由で鳴り、render_mix に統合される。プリセット JSON がステートとして効く | ✅ フィクスチャ CLAP シンセで実証（既定 440Hz/ピーク 0.5、プリセット `{"gain":0.25,"square":true}` でピーク 0.25＋波形変化）。drive の CLI 書き出しでも 440Hz＋523.25Hz・ピーク 0.250 を確認 |
| 3 | プラグインが無い／起動失敗／プリセットが読めないとき、無音でなく明確なエラー状態になり、アプリは落ちず、他トラックの再生を壊さない | ✅ 異常系3パターン（プラグイン不在・プリセット不在・ヘルパー不在）をテスト。サンプラートラックが鳴り続けることもテストで確認。GUI はトランスポートに `PLUGIN ERROR [トラック名] 理由` を表示 |
| 4 | エラー状態が GUI と CLI の両方で観測できる | ✅ GUI: 起動時 stdout＋トランスポート表示（スクリーンショット）。CLI: --export-wav の stderr に出力し、書き出し自体は成功（終了コード 0） |

## 前提スパイクの結論（重要）

- **Vital 実バイナリでの検証は未実施（needs-human 相当）**。公式ダウンロードは vital.audio の無料アカウント経由配布で、この VM では入手できなかった（GitHub Releases にもバイナリ無し）。ユーザーが Vital 本体を入手した環境で、実プラグインでの読み込み・プリセット適用の確認が別途必要。
- 代替としてワークスペース内にテスト用 CLAP シンセ（`fixtures/clap-test-synth`）を作り、パイプライン全体（別プロセス起動→CLAP ステート拡張でプリセット JSON 注入→ノート列レンダリング→PCM 受け渡し→render_mix 統合→WAV 書き出し）を実証した。
- ADR-0017 の差し替え条件（公式 Linux 版 Vital に CLAP が無い等）に触れる事実は判明しなかった（KVR 等で CLAP 対応が案内されている）。

## TDD の記録

- コア: Plugin バリアント関連テストが `PluginFormat`／`Instrument::Plugin` 未定義でコンパイルエラー（red）→ モデル＋検証を実装して green。
- プラグイン連携: plugin::tests 6件中5件がスタブ実装（未実装エラー）で失敗（red）→ ヘルパー／クライアント実装で green。残り1件（サンプラー無視）は最初から green。
- render_mix 統合: プラグイン再生テストが実装前に失敗（red）→ 統合して green。
- レビュー対応（第1ラウンド Major）: 指紋テスト2件が `plugin_fingerprint` 未定義でコンパイルエラー（red）、preset 空白テストが検証不足で失敗（red）→ 実装して green。監視経路の再現は drive.sh の GUI 駆動で確認（エラー帯のアクセント画素 456 → 0）。

## レビュー

- 第1ラウンド: CHANGES_REQUESTED（Major 1件: 事前レンダリングの無効化漏れ／Minor 2件: preset_path 空白検証の非対称、ヘルパー note_id への音高流用）。いずれも再現を確認して修正（コミット 536268a）。Major は指紋方式で全経路を一括無効化し、drive.sh に監視経路の再現検証を追加した。
- 第2ラウンド: **APPROVE**（新たな Major/Minor なし）。受け入れ条件を超える候補4件（二重レンダリング、指紋のフレーム毎 I/O、note_id 回帰検出、バイナリ差し替え未検知）はフォローアップ #75 に切り分けた。

## 証拠

- `docs/test-reports/evidence/issue-0072/drive.sh`（CLI 書き出し検証＋GUI 駆動＋外部変更の監視経路検証）
- `shot1-timeline-plugin-error.png`: GUI トランスポートの PLUGIN ERROR 表示
- `shot2-after-external-fix.png`: 起動中にプロジェクトファイルを書き換えて直した後、エラー表示が消えた状態
- `export-stdout.txt` / `export-stderr.txt` / `gui-stdout.txt` / `gui-stderr.txt`
- GUI 証拠は Xvfb ＋ lavapipe のソフトウェア描画。実ディスプレイ／実 GPU での見え方は未検証。

## テスト件数

- 着手前: ワークスペース合計 333 件
- 実装後: 347 件（コア +5、DAW +8、MCP +1）
- レビュー対応後: 349 件（指紋テスト +2）
