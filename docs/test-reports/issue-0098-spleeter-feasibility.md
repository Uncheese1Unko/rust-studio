# 試験レポート: Issue #98 調査 — Spleeter がこの環境で動くか＋ピッチ追跡の答え合わせ

- 日付: 2026-11-26
- イシュー: #98（EPIC #97 の調査 PBI）
- 結論: **Spleeter はこの VM の Python 3.12 では動かない**（証拠を下記に揃えた）。`needs-decision` で止め、代替案の比較を ADR-0024 に残した。ピッチ追跡の試作は答え合わせを通過した。
- 本番コードの変更: なし（調査のみ）。`cargo test --workspace` は 504 件全通過で不変。

## 受け入れ条件ごとの結果

### 1. 専用 venv に spleeter をインストールできる — ❌（正規手順では不可）

- venv: `~/spleeter-venv`（Python 3.12.3、リポジトリ管理外）。
- `pip install spleeter` の結果: pip は spleeter **2.1.0** を解決し（最新の 2.4.2 は `requires_python: <3.12,>=3.8` のため 3.12 では候補から外れる）、依存する旧 numpy のソースビルドで失敗した。
  - エラー: `NameError: name 'CCompiler' is not defined`（`numpy/distutils` 由来。distutils は Python 3.12 で削除）。
  - ログ: `evidence/issue-0098/pip-install-spleeter.log`
- spleeter 2.4.2 の宣言（PyPI メタデータで確認）: `requires_python: <3.12,>=3.8`、依存は `tensorflow==2.12.1`（対応 Python 3.8〜3.11）、`pandas<2.0.0,>=1.3.0`、`numpy<2.0.0`、`tensorflow-io-gcs-filesystem==0.32.0` 等。

### 2. テスト WAV を実際に分離できる — ❌（強制構成でも推論で失敗）

依存を Python 3.12 対応版に寄せた強制構成を試した。

- 構成: `pip install --no-deps --ignore-requires-python spleeter==2.4.2` ＋ tensorflow 2.17.1、numpy 1.26.4、pandas 2.1.4、norbert 0.2.1、httpx 0.19.0、typer 0.3.2、ffmpeg-python 0.2.0、soundfile 0.14.0、tensorflow-io-gcs-filesystem 0.37.1。venv 容量 **2.3GB**。
  - 途中の失敗として記録: 初回は pip の一時領域が `/tmp`（tmpfs 512MB）で、601.4MB の TF ホイールが `No space left on device` になった。`TMPDIR` と `PIP_CACHE_DIR` をホーム側へ逃がして解消（TF 一式の導入に 3分51秒）。
- 到達点: `import spleeter` 成功。`Separator('spleeter:2stems')` の初期化でモデル（2stems.tar.gz）のダウンロード・チェックサム検証・展開まで成功（初期化 14.3 秒）。
- 失敗点: `separate_to_file` が推論開始直後に例外で終了した（全体で 29.1 秒）。
  - エラー: `AttributeError: module 'tensorflow' has no attribute 'estimator'`
  - 発生箇所: `spleeter/separator.py` の `create_estimator`（`tf.estimator.RunConfig`）。`tf.estimator` は TensorFlow 2.16 で削除済み。
  - ログ: `evidence/issue-0098/separate-attempt.log`、スクリプト: `evidence/issue-0098/separate_attempt.py`
- 追加の打ち手も確認済み: `tensorflow-estimator` の最終版 2.15.0 は TF 2.17 に対して import 自体が失敗する（`ImportError: cannot import name 'estimator_training' from 'tensorflow.python.distribute'`）。estimator を含む TF 2.15 系には Python 3.12 用ホイールが無い。

### 3. ピッチ追跡の試作が音高列を返す — ✅（分離ステムの代わりに単体レンダリングで答え合わせ）

分離が成立しなかったため、分離後ステムそのものへの適用はできなかった。代わりに、メロディ単体のレンダリングと、伴奏付きミックスの2入力で試作を検証した（試作コード: `evidence/issue-0098/pitch-proto-main.rs`、外部クレートなし、自己相関＋放物線補間、窓 2048／ホップ 512、最小ノート長 4フレーム）。

テスト WAV（`evidence/issue-0098/make_test_wav.py` で決定的に生成、44.1kHz ステレオ）: メロディは A4（440Hz）→C5（523.25Hz）→E5（659.25Hz）→A5（880Hz）を各 500ms（正弦波＋2倍音・3倍音、減衰エンベロープ）。伴奏は C3／E3／G3 のパッド。

| 入力 | 抽出結果 | 判定 |
|---|---|---|
| メロディ単体 | 69 @0ms／476ms、72 @487ms／487ms、76 @986ms／487ms、81 @1497ms／464ms | **4音すべて音高一致、開始誤差は最大 14ms**（許容 ±100ms 以内） |
| 伴奏付きミックス | 前半は伴奏の低音（50、53）に引きずられ、後半の 76／81 は正しく抽出 | 分離なしのミックスへの直接適用は不十分（想定どおり。追跡は分離後ステムに掛ける設計） |

### 4. 方式を ADR に記録する — ✅

ADR-0024（`docs/adr/0024-stem-separation-runtime.md`）を作成した。骨格（別プロセス実行、原子的取り込み、生成物の置き場、失敗時の扱い）は維持し、ツール／実行環境の選択は選択肢 A〜D の比較表とともに判断待ちとした。

### 5. 動かない場合の証拠と代替案 — ✅

- 本レポートと ADR-0024 に証拠（バージョン制約、エラーメッセージ、発生箇所）と代替案比較（A: Python 3.11 venv、B: Demucs、C: ステム取り込みのみ、D: Spleeter パッチ＝非推奨）を残した。
- 補足の事実確認: Demucs 4.1.0 は PyPI メタデータで `requires_python >=3.10`、依存 `torch>=2.1`（torch は 3.12 用ホイールあり）→ 3.12 で入る見込み（実インストールは未検証）。Python 3.11 は VM に無く、apt にも候補が無い（導入には uv のスタンドアロン CPython や PPA が必要）。
- Issue #98 に `needs-decision` ラベルを付けた。EPIC #97 は open のまま。

## TDD の記録

調査 PBI のため通常の red/green は無い。代わりに検証の当て方を記録する。

- Spleeter: 「インストール→分離実行」が受け入れ条件そのもので、上記のとおり失敗を実測で確定した（失敗は3経路とも再現性あり）。
- ピッチ追跡試作: 期待する音列（69, 72, 76, 81／各 500ms 間隔）を先に固定し、試作の出力と機械的に照合した（表のとおり）。

## レビュー

コード変更（本番）が無いため、delivery-loop §1-7 により独立コードレビューは省略した。追加した文書は ADR-0024 と本レポート、証拠ファイルのみ。

## ゲート

- `cargo test --workspace`: 504 件全通過（変更前と同一。本調査で本番コードは変更していない）。
- CI: 本レポートと ADR の push 後に確認し、結果を Issue #98 のコメントに残す。
