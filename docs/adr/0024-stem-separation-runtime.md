# ADR-0024: 音声分離の実行環境 — Spleeter は Python 3.12 では動かず、方式は判断待ち

- ステータス: 提案（needs-decision、Issue #98）
- 日付: 2026-11-26
- 決定者: ユーザー（判断待ち）。調査と提案: エージェント（Issue #98）
- 関連: ADR-0016（ノート／音色モデル）、ADR-0017（外部ツールの別プロセス分離）、ADR-0018（音色コレクション）、ADR-0006（設計判断は人間が持つ）

## 背景

EPIC #97（WAV を Spleeter で分離し、各ステムをメロディと音色に分解する）の前提として、開発 VM で Spleeter が実際に動くかを Issue #98 で実証した。VM の Python は 3.12.3 で、Spleeter／TensorFlow は未インストールだった。

調査の結論: **Spleeter はこの VM の Python 3.12 では、正規のインストールでも依存を寄せた強制構成でも動かない。** 証拠は次のとおり（詳細とログは試験レポート issue-0098）。

1. **正規インストール不可**: 最新の spleeter 2.4.2 は `requires_python: <3.12,>=3.8` を宣言し、`tensorflow==2.12.1`（対応 Python は 3.8〜3.11）と `pandas<2.0.0` を固定する。Python 3.12 の pip は 2.4.2 を解決できず spleeter 2.1.0 まで落ち、その依存する旧 numpy がソースビルドになり、`numpy.distutils`（Python 3.12 で削除）由来の `NameError: name 'CCompiler' is not defined` で失敗する。
2. **強制構成でも推論不可**: spleeter 2.4.2 を `--no-deps --ignore-requires-python` で入れ、依存を Python 3.12 対応版に寄せた構成（tensorflow 2.17.1、numpy 1.26.4、pandas 2.1.4、norbert 0.2.1 等、venv 容量 2.3GB）では、`import spleeter` とモデル（2stems）のダウンロード・展開までは成功した。しかし分離実行が `spleeter/separator.py` の `create_estimator` 内 `tf.estimator.RunConfig` で `AttributeError: module 'tensorflow' has no attribute 'estimator'` となり失敗した。`tf.estimator` は TensorFlow 2.16 で削除されている。
3. **estimator の単独提供も不可**: `tensorflow-estimator` パッケージは 2.15.0 が最終で、TF 2.17 に対しては内部 API（`tensorflow.python.distribute.estimator_training`）の削除により import 自体が失敗する。estimator を含む最後の TensorFlow 2.15 系は Python 3.12 用ホイールが存在しない（対応は 3.11 まで）。

つまり障壁は「バージョンの組み合わせ」ではなく、**Spleeter の推論経路そのものが削除済み API（tf.estimator）に依存している**ことにある。これを回避するには Spleeter 内部の推論実装を書き換える（フォーク保守）しかなく、製品に抱えるには脆すぎる。

なお、分解の後半（ピッチ追跡）は Spleeter と独立に実証できた。自己相関ベースの試作（外部クレートなし）は、既知メロディ（A4→C5→E5→A5、各 500ms）の単体レンダリングに対し **4音すべて音高一致・開始誤差 14ms 以内** で抽出できた。分離方式が決まれば、#100（メロディ抽出）はこの試作を土台に進められる。

## 決定

**未決定（人間の判断待ち）。** ツールの乗り換えは設計判断（ADR-0006）のため、エージェントは下記の選択肢を提示して止まる。決まるまでは #99〜#101 に着手しない。

分離の骨格そのもの（DAW から別プロセスで実行し、結果をコマンド列で原子的に取り込む、ADR-0017 と同じ考え方）は選択肢によらず維持する。生成物（ステム WAV）はプロジェクトに紐づく生成物ディレクトリに置き、失敗時はプロジェクト状態を変えず、エラーを利用者へ見える形で返す。

## 検討した選択肢

| 選択肢 | 内容 | 長所 | 短所 |
|---|---|---|---|
| A. Python 3.11 の venv を別途用意し Spleeter を使う | uv のスタンドアロン CPython 等で 3.11 を導入し、spleeter 2.4.2 + tensorflow 2.12.1 の設計どおりの環境を作る | ユーザーが指定した Spleeter をそのまま使える。バージョンが設計どおりで最も確実 | VM に python3.11 が無い（apt に候補なし。uv 導入か PPA 追加が要る）。TF 2.12 系は旧く、保守されなくなる一方。venv 約2.3GB |
| B. Demucs に乗り換える | PyTorch 系の分離ツール demucs 4.1.0（`requires_python >=3.10` で 3.12 対応、torch>=2.1） | 現行 Python 3.12 のまま入る見込み（メタデータ確認済み、実インストールは未検証）。分離品質は Spleeter より高いとされる。4ステム（vocals/drums/bass/other）が標準 | ユーザー指定からの乗り換え。torch が大きく（CPU ホイールで約800MB級）初回モデル取得も要る。実行時間は Spleeter より重い傾向 |
| C. 外部で分離したステムの取り込みのみ対応する | 分離はユーザーが手元の環境や外部サービスで行い、DAW は「ステム WAV 群の取り込み→メロディ抽出→音色化」だけを実装する | ML 実行環境を製品が抱えずに済む。#100・#101 の価値はそのまま届く | 「WAV を読み込ませたら自動で分離」という体験にはならない。手間がユーザーに移る |
| D. Spleeter の推論経路をパッチして 3.12 で動かす | `create_estimator` を tf.estimator なしで書き換えたフォークを保守する | Spleeter の名前とモデルはそのまま | ライブラリ内部の書き換え保守が恒常化する。TF／Keras の更新のたびに壊れ得る。**非推奨** |

推奨は **A**（ユーザー指定の Spleeter を設計どおりの環境で使うのが最も素直で、3.11 の導入手段も uv のスタンドアロン配布で閉じられる）か、3.12 に残ることを優先するなら **B**。

## 結果

- 判断が決まるまで EPIC #97 は open のまま、#98 は `needs-decision` で止まる。#99〜#101 は blocked のまま着手しない。
- 判断が決まったら、本 ADR を「採用」に更新し（または新 ADR で supersede し）、決まった方式の骨格（ステム数、プロセス連携、生成物の置き場、失敗時の扱い）を追記してから #99 に着手する。
- ピッチ追跡の試作と答え合わせ結果は方式によらず有効で、#100 の土台になる。
- 覆す条件: Spleeter が tf.estimator 依存を外した新版を出し、Python 3.12 以降に対応したら、選択肢は再評価する。
