# 試験レポート: #75 プラグイン事前レンダリングの効率・堅牢性フォローアップ

対象 PBI: #75「プラグイン事前レンダリングの効率・堅牢性フォローアップ（#72 レビュー由来）」
実施日: 2026-11-24
実装コミット: `63cf5b8`
ベース: `0f5247c`

イシュー本文の候補1〜4をすべて実施した。

## 実装内容

- **候補1（指紋の同期漏れによる二重レンダリング）**: `refresh_audio_map(model)` を新設し、音声マップの再計算と同時に `model.plugin_fingerprint` を同期する。Model を持つ全呼び出し箇所（外部再読込、ドロップ、Undo/Redo、配置、音色クリック、`update()` の指紋変化経路）をこのヘルパー経由に統一した。残る直接呼び出しは `model()` 初期化（初期指紋を同じキャッシュで計算して保持）と `export_wav_cli`（Model を持たない一回限りの CLI で、二重レンダリングは起こらない）のみ。
- **候補2（指紋計算のフレーム毎ディスク I/O）**: `plugin::FileHashCache` を追加。パス→（サイズ, mtime, ハッシュ）を保持し、両者が一致する間はファイルを読み直さない。`update()` の毎フレーム指紋計算は `plugin_fingerprint_cached` 経由になった。固有限界として、同サイズかつ mtime が保存される差し替え（`cp --preserve` 等）は検出できない（候補2自体が提案した方式のトレードオフ。レビュー任意指摘として記録）。
- **候補4（プラグインバイナリ自体の差し替え未検知）**: 指紋文字列に `bin#<内容ハッシュ>;`（読めなければ `bin#missing;`）を追加し、プラグインバイナリの内容も指紋に含めた。
- **候補3（note_id 修正の回帰検出）**: フィクスチャシンセ（`fixtures/clap-test-synth`）のボイスが `note_id` を保持し、ID 指定のノートオフは一致するボイス1つだけを止めるようにした（ID 無しの場合は従来どおりキーで止めるフォールバックを残した）。これにより、同音高の重なりを実レンダリングで検証できるようになった。
- あわせて `update()` のプラグインエラーログは、整形済み文字列（`PLUGIN ERROR [track] msg`）をそのまま出す形に変わった（表示のみの差）。

## TDD の記録（red）

1. 候補2・4: 新規テスト（`fingerprint_changes_when_plugin_binary_changes`、`cached_fingerprint_matches_and_detects_preset_change`）を先に書き、新規 API を一時的に無効化した状態で E0425／E0433（`plugin_fingerprint_cached`／`FileHashCache` が無い）のコンパイル失敗を確認した。
2. 候補3: 新規テスト `prepare_keeps_overlapping_same_pitch_notes_independent`（同音高69の重なり 0–500ms と 250–750ms）を書き、フィクスチャを旧実装（音高キーでボイス管理）に戻した状態で実行し、`second note must survive the first note-off, peak was 0` のアサーション失敗を確認した。フィクスチャ修正後に green。レビュアーも変異テストで同じ落ち方を再現し、検出力を裏付けた。

## 単体テスト

- `cargo test -p daw`: 148 件 → 151 件（新規3件）。
- ワークスペース全体: 380 件 → 383 件、全て通過。
- `./scripts/verify.sh`: fmt / clippy / テストすべて OK。

## 実アプリ駆動（GUI 証拠）

本変更は内部効率・堅牢性の改善で、画面で観測できる差分はない。Xvfb（display :97）＋ lavapipe で実アプリの起動スモークのみ実施し、`elements:` の出力と10秒間の継続動作を確認した（標準出力ログ: `evidence/issue-0075/gui-stdout-smoke.txt`）。**実ディスプレイ／実 GPU での見え方は未検証。**

## 環境の出来事

実施中に VM の libclang が消失し（apt の os-intent replay が長時間実行中だった）、bindgen を要する再ビルドが一時失敗した。replay 完了後に `/usr/lib/llvm-18/lib/libclang.so` の復旧を確認して再開した。コード側の問題ではない。

## 独立レビュー

- ラウンド1: APPROVE。候補1〜4の充足、既存の指紋の意味論（プリセット欠落＝`preset#missing;`、音量・ゲインを含めない）が壊れていないこと、スコープ外混入・ゲート弱体化がないことが確認された。任意指摘は、候補2方式の固有限界（同サイズ＋mtime保存の差し替えは未検出）と `update()` のエラーログ形式の変化で、いずれも本レポートに記録した。

## Definition of Done チェック

- [x] イシューの候補1〜4をすべて実施
- [x] TDD（red を記録。候補3は振る舞いの red を取得）
- [x] `./scripts/verify.sh` 緑
- [x] 独立レビュー APPROVE（1ラウンド）
- [x] 起動スモークで証拠を取得（画面差分のない変更のため、証拠はテストと起動ログが中心）
- [x] 試験レポートと証拠を配置
- [x] CI 緑（push 後に確認）
