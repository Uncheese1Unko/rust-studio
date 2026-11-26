# 試験レポート: Issue #99 WAV 取り込みから Demucs 分離を実行し、ステムをプロジェクトに追加できる

- 日付: 2026-11-26
- イシュー: #99（EPIC #97 の PBI）
- 結論: **全受け入れ条件を満たした。** Demucs の実証（venv 構築＋実分離）、
  CLI 駆動（成功系・失敗系）、GUI 駆動（XDND ドロップ→分離→適用）まで確認した。
- テスト件数: ベースライン 504 件 → 519 件（新規15件、separation。
  うち1件はレビュー round 1 の指摘対応で追加）。
  削除・弱体化なし。
- 環境: Yoko の VM（Linux、ヘッドレス）。GUI 確認は Xvfb（:98）＋
  Mesa lavapipe（ソフトウェアレンダリング）。**実ディスプレイでの
  確認は未検証。**

## Demucs の実証（受け入れ条件1）

**結論: Demucs はこの環境で実際に動く。** 専用 venv で短いテスト WAV を
分離し、4ステムが生成されることを確認した。

- venv: `~/demucs-venv`（Python 3.12.3）。容量 **1.1GB**。
  - torch 2.11.0+cpu／torchaudio 2.11.0+cpu（CPU ホイール）、
    demucs 4.1.0、numpy 2.5.5（demucs が import するが依存宣言が
    無いため明示追加した）。
  - 導入時の実測上の注意: この VM のプロキシ経由では Python の
    HTTP クライアント（pip／urllib／httpx）からの大きなファイルの
    取得が極端に遅くタイムアウトを繰り返した。torch／torchaudio の
    ホイールとモデルチェックポイントは curl で取得してから
    ローカル／キャッシュへ入れることで回避した（PyPI からの通常の
    pip インストールは問題なく通った）。製品コードの責務ではなく
    venv 運用の注意として記録する。
- モデル: htdemucs のチェックポイント `955717e8-8726e21a.th`
  （約81MB、torch hub キャッシュに配置）。
  - Demucs 4.1 はモデル取得にまず HuggingFace を試みるが、この
    環境では応答待ちで長時間（30分超）止まることを実測した。
    同じ重みは従来経路（dl.fbaipublicfiles.com／torch hub
    キャッシュ）から得られるため、`run_demucs` は子プロセスの
    環境変数 `HF_HUB_OFFLINE=1` を設定して即座に従来経路へ
    落とす（実装判断。コード内コメントにも明記）。
- 実分離: テスト WAV（44.1kHz ステレオ 2秒。メロディ
  A4→C5→E5→A5＋C メジャーのパッド、#98 のレシピを純 Python で
  再生成）に対し、`python -m demucs -n htdemucs -d cpu` が
  **19.2秒**（プロセス起動・モデル読み込み込み、キャッシュ温済み）
  で終了し、`<out>/htdemucs/input/{vocals,drums,bass,other}.wav`
  の4ステム（各 352,844 バイト）を生成した
  （証拠: `evidence/issue-0099/separate2.txt`、`make_test_wav.py`）。

## 受け入れ条件ごとの結果

### 1. Demucs がこの環境で実際に動く — ✅

上記「Demucs の実証」のとおり。venv 容量 1.1GB、分離所要時間
19.2秒（2秒のテスト WAV、キャッシュ温済み）を実測した。

### 2. WAV を取り込むと4ステムが新規トラック＋波形クリップで追加される — ✅

- CLI 駆動（`daw --separate project.json input.wav`、#70 の種
  プロジェクトに MIDI 取り込みで Lead／Bass を入れたもの）:
  終了コード 0、「separated: 4 tracks added」。保存された
  プロジェクトに `input / vocals`、`input / drums`、`input / bass`、
  `input / other` の4トラックが追加され、各クリップの `source` が
  実在するステム WAV の**絶対パス**
  （`<作業場所>/project.stems/input-<hash>/htdemucs/input/<stem>.wav`）
  を指し、長さは実尺 2000ms、ノートは空（#99 は波形クリップまで）
  であることをダンプで確認した。絶対パスであることはレビュー
  round 1 の指摘で担保したもので、別 cwd（`/`）から開いても
  全ステムが解決できることを再駆動で確認済み
  （証拠: `evidence/issue-0099/separate-cli.txt`）。
- GUI 駆動: XDND ドロップで配置と同時に分離がワーカースレッドで
  始まり（stdout: `separation started:`）、完了後に
  `separated: drop (4 tracks)` が出て、保存されたプロジェクトと
  タイムライン表示に4ステムのトラックが現れた
  （証拠: `evidence/issue-0099/drive.sh`、`gui-stdout.txt`、
  `shot1-separated.png`）。

### 3. 分離前の元 WAV のトラックは従来どおり残る — ✅

CLI 駆動で分離前後のプロジェクト JSON を比較し、既存トラック
（Lead／Bass）がバイト等価に不変であることを確認した。GUI でも
ドロップ配置されたクリップと既存トラックがそのまま残っている。

### 4. 適用は原子的で、Undo 1回で分離前に戻れる — ✅

単体テスト `apply_stems_adds_waveform_track_and_clip_per_stem`
（適用→Undo→ダンプ一致）と
`apply_stems_failure_leaves_project_and_history_untouched`
（不正なステムで全体が失敗し、プロジェクト・履歴とも不変）で
確認した。実装は MIDI 取り込みと同じく、複製上でコマンド列を
適用してから `History::commit_replacement` で差し替える。

### 5. Demucs が失敗／未導入のとき、アプリは落ちず、エラーが見え、状態が変わらない — ✅

- 壊れた WAV（テキストファイル）を CLI に渡す: 終了コード 1、
  `separation failed: demucs separation failed: ... no suitable
  format reader found ...` のメッセージ、プロジェクト JSON は
  不変（証拠: `evidence/issue-0099/fail1.txt`）。
- venv が無い環境設定（`RUST_STUDIO_DEMUCS_PYTHON` に存在しない
  パス）: 終了コード 1、`demucs is not installed: ...` の明示
  エラー、プロジェクト JSON は不変
  （証拠: `evidence/issue-0099/fail2.txt`）。
- GUI では未導入・失敗がステータス表示（SEPARATION UNAVAILABLE／
  SEPARATION FAILED、失敗はピンク）と stdout に出る実装で、
  解決失敗時にワーカーを起動しないこともコードで担保した
  （実アプリでの失敗系表示は CLI と単体テストで代替確認）。

### 6. 実行方式は ADR-0024 に従う — ✅

venv 解決（環境変数→規約パス）、生成物の置き場
（`<プロジェクト名>.stems/<元ファイル名>-<元パスのハッシュ>/`、
デモモードは `media_dir()/stems/`）、原子的適用、GUI の
ワーカースレッド実行＋`update()` での受け取り、のいずれも
ADR-0024 の骨格どおりに実装し、単体テスト
（`resolve_python_*`、`stems_root_*`、`separation_out_dir_*`、
`stem_output_path_*`）で固定した。

## TDD の記録

### separation.rs（分離の解決・配置・適用）

- テスト13件を先に書き、`todo!()` の本体で実行: **13件すべて失敗**
  （`not yet implemented` の panic）。期待した理由（未実装）での red を確認。
  - 対象: `resolve_python`（環境変数優先／不在は NotInstalled／venv 規約
    パスへのフォールバック／venv 無しは NotInstalled）、`check_demucs`
    （import 失敗の再現=/bin/false は NotInstalled、0 終了=/bin/true は Ok）、
    `stems_root`（プロジェクト同階層／デモモード）、`separation_out_dir`
    （決定的・パス感受性）、`stem_output_path`（Demucs の出力レイアウト）、
    `run_demucs`（存在しない Python は panic でなくエラー）、
    `apply_stems`（4ステム分のトラック＋波形クリップ追加、既存トラック
    不変、Undo 1回で復元、失敗時はプロジェクト・履歴とも不変）。
  - 実装後: 13件すべて green。
- `separate_file` のテスト（/bin/true で「実行は 0 終了だがステムを
  作らない」場合に出力不足の Failed になる）を先に書き、`todo!()` で
  失敗する red を確認 → 実装して green。
- レビュー round 1 の指摘対応でも TDD を踏んだ:
  `stems_root_absolutizes_a_relative_project_path` を先に書き、
  絶対化前の実装で落ちる（red）ことを確認 → `stems_root` で
  プロジェクトパスを絶対化して green。あわせて CLI 駆動を再実行し、
  別 cwd から全ステムの `source` が解決できることを確認した。

## コードレビュー

- Round 1: **CHANGES_REQUESTED**（ブロッキング1件）。
  - 指摘: プロジェクトパスが相対で渡されると、保存されるステムの
    `source` が cwd 相対になり、別 cwd からは解決できず受け入れ
    条件2を満たさない。→ `stems_root` でプロジェクトパスを
    絶対化して対応（上記 TDD の記録）。CLI 証拠も絶対パスで
    取り直した。
  - 非ブロッキング所見（対応せず）: GUI で「Python は解決できるが
    demucs を import できない」場合は UNAVAILABLE でなく FAILED と
    表示される（import 確認がワーカー内で走るため）。エラー自体は
    明示され状態も壊れないため、ラベルの差として記録に留める。
  - その他の受け入れ条件・コマンド層経由・原子性・失敗系・
    ADR-0024 適合はレビュアーが実測込みで確認済み。
- Round 2: **APPROVE**。レビュアーは絶対化の修正を回帰テストと
  証拠ログの実ファイルで再確認し、全受け入れ条件・コマンド層
  経由・原子性・失敗系・ADR-0024 適合を独立に検証した
  （ワークスペース 519 件全通過もレビュアー側で実測）。
