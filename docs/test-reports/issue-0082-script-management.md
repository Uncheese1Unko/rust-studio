# 試験レポート: #82 スクリプト／拡張のインストールと管理ができる

対象 PBI: #82「スクリプト／拡張のインストールと管理ができる（配置で認識・一覧・有効／無効）」
実施日: 2026-11-24
実装コミット: `38b364d`（初版）、レビュー対応コミットは本レポートと同梱（Refs #82）
ベース: `82f5f36`

## 実装内容

- **カタログ（共有クレート `crates/studio-scripts` の `catalog`）**: `scan_scripts(dir)` がスクリプトフォルダ（`media::scripts_dir`）を走査し、`.lua` を名前順に一覧化する。各エントリは有効／無効と状態（Ready／Error）を持つ。壊れたスクリプトは `ScriptEngine::check_syntax`（コンパイルのみ・実行しない）で構文エラーを判定し、Error として隔離する（他のスクリプトの読み込みは妨げない）。`reference_statuses(project, dir)` はプロジェクトのクリップ参照を畳んで Ready／Disabled／Error／Missing を返し、欠落を明示する。`dump_json` が観測用ダンプ（GUI 標準出力と MCP で同一形状）。
- **有効／無効の保持先（判断）**: イシューの第一候補どおり**アプリ側設定**とした。スクリプトフォルダ内の `disabled-scripts.txt`（1行1名、名前順）に無効な名前だけを保持する。理由: (1) 有効／無効は「この環境に入れた拡張を止める」というローカルな運用判断で、プロジェクトの内容ではない。プロジェクトファイルに入れると、共有した相手の環境でも無効化されてしまい、参照（名前）のみを持つ #81 の設計ともずれる。(2) フォルダ内に置くことで、一覧・無効リスト・本体が同じ場所で完結し、フォルダごと持ち運べる。プロジェクト側は従来どおり参照のみで、本体は埋め込まない。
- **適用経路（media.rs）**: `apply_script_effect_with_dirs` は解決したファイル名で無効リストを引き、無効なら「スクリプト X は無効化されているため適用できません」を返す。プレビュー／書き出し共通の単一経路（#81）のため、両方で同じく適用されず、GUI は既存のエラーラベルにその旨を表示する（適用済みクリップで無効表示になる）。
- **GUI（main.rs）**: メディアビンの下にスクリプトパネル（要素識別子 `script-list`）を追加。行は状態接頭辞で区別（[on]／[off]／[error]／[missing]、エラー系は赤、無効は灰）。行のダブルクリックで有効／無効を切り替え（欠落行は表示のみ）。起動時・プロジェクト再読込時・ビン再読み込み（R キー／ヘッダー）・L キーでの参照変更時に一覧と参照状態を取り直し、`scripts: {json}` と `missing scripts: ...` を標準出力に出す。
- **MCP（studio-mcp）**: 読み取り専用ツール `list_scripts` を追加。走査・状態・参照の判定は共有クレートの単一実装を使い、GUI ダンプとまったく同じ JSON を返す（参照状態はサーバー保持プロジェクトから判定）。`dir` 引数でフォルダを上書きできる。
- **構文検証（engine）**: `ScriptEngine::check_syntax` を追加（`lua.load(...).into_function()` でコンパイルのみ）。実行時エラーや無限ループを含むソースでも構文が正しければ即座に Ok を返すことをテストで固定した。

## レビュー対応での構造変更（round 1 Major 指摘）

初版はカタログを video-editor 内のモジュールに置き、MCP 側は名前＋有効／無効だけを返す簡易走査を複製していた。独立レビュー round 1 が「MCP で壊れたスクリプトと欠落が観測できず、走査ロジックの複製は architecture ルール（ラッパーにロジックを書かない）にも反する」と指摘（Major）。対応として、Lua 基盤（旧 `apps/video-editor/src/script.rs`）とカタログ（旧 `extensions.rs`）を共有クレート **`crates/studio-scripts`**（`engine`／`catalog`）へ移し、video-editor と studio-mcp が同じ実装を使う形にした。`resolve_script_in` も catalog へ移し、media 側は再公開する。MCP の `list_scripts` は `dump_json` の完全な形を返すようになった。

## 受け入れ条件の確認

- [x] 所定フォルダに `.lua` を置くと再起動後に一覧へ現れる — `scan_scripts` のテスト＋ GUI ドライブ（配置→起動→ダンプとパネルで確認）
- [x] 一覧が GUI と状態ダンプ／MCP の両方で観測できる — GUI パネル＋標準出力 `scripts:` ダンプ（ドライブ）、MCP `list_scripts`（mcp_tests で GUI と同じ形・状態・参照まで確認）
- [x] スクリプトごとに有効・無効を切り替えられ、無効なスクリプトは適用できない（適用済みクリップでは無効表示） — `set_enabled` 往復テスト、media の `script_effect_disabled_script_is_not_applied`（無効→エラー、再有効化→画素が実際に反転）、ドライブ（ダブルクリック切替→ダンプ変化→適用エラー表示）
- [x] プロジェクトは参照のみで持ち、スクリプトが見つからないプロジェクトを開くと欠落が明示される — `reference_statuses` テスト（Missing）、ドライブ（`missing scripts: ghost.lua` 行とパネルの [missing] 行、起動時ダンプの references）
- [x] 壊れたスクリプトが一覧でエラーとして区別でき、他のスクリプトの読み込みを妨げない — `scan_isolates_broken_script_as_error`、ドライブ（[error] broken.lua 行とダンプの status.error）

## TDD の記録（red）

1. 構文検証: テスト3件を先に書き、`check_syntax` 未実装でコンパイルエラー（E0599）になることを確認。実装後 green。
2. カタログ: テスト7件を先に書き、全関数 `todo!()` で全件失敗（"not yet implemented"）を確認。実装後 green。
3. 無効スクリプトの適用拒否: media のテストを先に書き、無効化しても適用されてしまう（Ok が返る）ことで red を確認。適用経路に無効チェックを入れて green。
4. MCP: `list_scripts` のテストと tools/list の期待名を先に書き、ツール不在で2件 red を確認。実装後 green。round 1 対応ではテストを「GUI と同じ形（status／references）」へ拡張し、共有化前の実装で落ちることを確認してから studio-scripts へ移した。
5. レビュー Minor（無効＋エラーの併存でエラーが見えない）: `display_rows_prioritize_error_over_disabled` を先に書いて red（Disabled が返る）を確認し、優先順位を直して green。

## 単体テスト

- `crates/studio-scripts`: 36 件（engine 26、カタログ 10。round 2 対応の回帰テスト2件を含む）。
- `crates/studio-mcp`: 33 件（+1: list_scripts、tools/list の期待名にも追加）。
- `apps/video-editor`: 101 件（二モジュール移設分を除く。media の無効適用テストを含む）。
- ワークスペース全体: **440 件**すべて通過。`./scripts/verify.sh`: fmt / clippy / テストすべて OK。

## 実アプリ駆動（証拠）

- `docs/test-reports/evidence/issue-0082/drive.sh`（Xvfb :96 + lavapipe、実ディスプレイ／GPU での見え方は未検証）: DRIVE OK。
  - 配置した3スクリプト（正常 invert.lua／構文エラー broken.lua／無効化済み off.lua）が起動時ダンプとパネルに出る（[on]／[error]／[off]）。
  - プロジェクトが参照する不在の ghost.lua が `missing scripts: ghost.lua` と [missing] 行で明示される。
  - off.lua 行のダブルクリックで有効化され、ダンプにも反映される。
  - invert.lua 行のダブルクリックで無効化され、再生ヘッドのクリップへの適用が止まり「スクリプト invert.lua は無効化されているため適用できません」がログとラベルに出て、アプリは生存する。
- 証拠ファイル: `drive.sh`、`drive-app.txt`、`panel.png`（切替前）、`panel-after.png`（切替後）。
- 注記: ダブルクリック駆動はソフトウェアレンダリング時のフレーム間隔で取りこぼしがあり、保持式の押下＋リトライで安定化した（アプリ側の不具合ではない）。

## 独立レビュー

- ラウンド1: CHANGES_REQUESTED。指摘3件:
  1. （Major）試験レポートがコミットに無い（DoD 違反）。→ 本レポートを作成し、対応コミットに同梱した。
  2. （Major）MCP の一覧が不完全（status／references が無い）で走査ロジックを複製している。→ 共有クレート `studio-scripts` へ基盤とカタログを移し、MCP は `dump_json` の完全な形を返すようにした（上記「構造変更」）。
  3. （Minor）パネル表示で無効がエラーを隠す。→ `display_rows` の優先順位をエラー優先に変更し、回帰テストを追加した。
- ラウンド2: CHANGES_REQUESTED。指摘4件、いずれも対応:
  1. （Major）共有クレート新設というアーキテクチャ判断に ADR が無く、architecture ルールのクレート構成も未更新。→ **ADR-0020** を作成し、`docs/adr/index.md` と `.claude/rules/architecture.md` のクレート構成に `studio-scripts` を追加した（判断を行った変更への記録として、レビュー対応コミットに同梱）。
  2. （Minor）試験レポートが `docs/log.md` と `docs/test-reports/index.md` に未登録。→ 両方に登録した。
  3. （Minor）`reference_statuses` が無効をエラーより優先し、パネル（display_rows）と観測がずれる。→ エラー優先に統一し、回帰テスト `reference_status_prioritizes_error_over_disabled`（red を確認してから修正）を追加した。
  4. （Minor）MCP の既定スクリプトフォルダがパス規約を複製している。→ 規約を `studio_scripts::default_scripts_dir` の単一の定義に寄せ、アプリの `media::scripts_dir` と MCP の既定値の双方が委ねる形にした（パス形状のテスト付き）。
- レビューは上限2ラウンドのため、round 2 指摘の修正は回帰テストと verify.sh で担保し、3ラウンド目は行っていない（修正は指摘の方向そのまま）。

## 判断の記録

- 有効／無効の保持先はアプリ側設定（スクリプトフォルダ内の `disabled-scripts.txt`）。理由は「実装内容」に記載。プロジェクト共有時の扱い: 無効化は共有されず、各環境の判断として残る（イシューの論点への回答）。
- カタログと Lua 基盤の置き場所は共有クレート `studio-scripts`（レビュー対応）。video-editor のバイナリクレートからは他バイナリが参照できないため、クレート化が単一実装の前提だった。
- MCP の `list_scripts` の参照状態は、サーバーが保持するプロジェクトで判定する。GUI と同じプロジェクトファイルを見ている場合は同じ結果になる。
- スクリプトのファイル名先頭 `@` 記法や .anm の一覧統合は #83 で扱う（SCRIPT_EXTENSIONS への "anm" 追加と構文検証の分岐が接続点）。

## Definition of Done チェック

- [x] 受け入れ条件をすべて、書いてある確認方法で確かめた
- [x] TDD（red を記録）
- [x] `cargo test` が 0 で終わった（ワークスペース 440 件）
- [x] 画面で観測できる受け入れ条件を実アプリ駆動＋スクリーンショットで確認した
- [x] 状態を変える受け入れ条件をダンプ（GUI ログ／MCP）で確認した
- [x] 独立レビューを受けた（2ラウンド、指摘はすべて対応）
