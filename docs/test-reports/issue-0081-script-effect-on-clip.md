# 試験レポート: #81 Lua スクリプトエフェクトをクリップに適用できる

対象 PBI: #81「Lua スクリプトエフェクトをクリップに適用できる」
実施日: 2026-11-24
実装コミット: `257a8e9`
ベース: `4c424a6`

## 実装内容

- **モデル／コマンド（studio-core）**: `Clip.script_effect: Option<ScriptEffect>` を追加（`#[serde(default)]` で旧 JSON 互換）。`ScriptEffect { script: String, params: BTreeMap<String, EffectParam> }`、`EffectParam` は無タグ enum（Number／Bool／Text、JSON では素のスカラー）。複数エフェクトの積み重ねはせず、フェード（#34）・トランジション（#49）と同じ「クリップの単一属性」モデル（#79 決定）。`Command::SetClipScriptEffect { clip_id, effect: Option<ScriptEffect> }` は検証してから適用する（空スクリプト参照・非有限の数値パラメータは `InvalidRange`、未知クリップは `ClipNotFound`、失敗時は状態不変）。分割時は右クリップがエフェクトを引き継ぐ（ゲイン／切り抜きと同じ扱い）。
- **MCP（studio-mcp）**: ツール `set_clip_script_effect` を追加（ディスパッチはコマンドの serde タグ経由で汎用）。HTTP API もコマンド経由で自動的に対応。
- **フレーム API（script.rs）**: `ScriptEngine::run_frame_effect(source, clip, params, width, height, pixels)` を追加。スクリプトへは `frame.width/height`、`frame.get(x,y)`、`frame.set(x,y,r,g,b[,a])`、`frame.fill_rect(...)`（クリッピング付き）を渡し、RGBA バッファを直接書き換える。`clip.time_ms`（クリップ相対時刻）も公開。サンドボックスと命令数上限は #80 の基盤をそのまま使う。
- **適用の共有規則（media.rs）**: `apply_script_effect(base, clip, at_ms, out_dir)` が唯一の適用経路。スクリプト参照は `scripts_dir()`（メディアディレクトリ配下 `scripts/`）で解決し（絶対パスはそのまま）、結果は「ベースパス＋スクリプト内容＋パラメータ＋寸法＋クリップ相対時刻＋クリップ名＋長さ」のハッシュでキャッシュする。プレビュー（main.rs）と書き出し（`compose_export_frame` の合成後段）はこの関数を共有し、規則の二重実装を避ける（ADR-0015 の「プレビュー＝書き出し」原則）。
- **GUI（main.rs / edit.rs）**: L キー（離しエッジ）で再生位置クリップのスクリプト名セッション、P キー（押下エッジ）で `key=value` パラメータセッション（`parse_param_entry`: 数値 > 真偽 > 文字列、`key=` は削除）。プレビューは切り抜き合成・ブレンドより優先してエフェクト済みフレームを表示し、ラベルにスクリプト名（エラー時はエラー内容）を出す。スクリプトエラー時はエフェクトなしで表示を続け、アプリは止まらない。

## 受け入れ条件の確認

- [x] エフェクト（スクリプト参照＋パラメータ）をクリップに設定でき、状態ダンプ（GUI／MCP）で観測できる — コア／MCP テスト、GUI ドライブ（L→`invert.lua` 入力→Enter でダンプに `script_effect`、P→`level=3` でダンプに `params.level = 3.0`）
- [x] プレビューにスクリプトの描画結果が反映される（ピクセル系で確認） — `script_effect_applies_to_frame_pixels`（反転）、GUI ドライブでスクリーンショットのプレビュー領域平均色がエフェクト済みファイルの平均色と一致し、そのファイルがベースフレームの反転であることを照合
- [x] mp4 書き出し（#54 の経路）に同じスクリプトが同じ意味で反映される — `compose_export_frame_applies_script_effect`（同一ベースに対する書き出しフレームとプレビュー経路のピクセル一致を表明）
- [x] スクリプトエラー（文法・実行時）でアプリが落ちず、そのクリップのエフェクトを無効化してエラーを表示する — `script_effect_missing_script_is_error`、`script_effect_broken_script_is_error`、GUI ドライブ（`bad.lua` で `script error: ... drive bad script` が出てアプリ生存、ラベルにも表示）
- [x] プロジェクト保存→再読込でエフェクト設定が残る — コア往復テスト（旧形式 JSON の読み込み・None での解除を含む）、GUI ドライブでプロジェクトファイル実物を確認

## TDD の記録（red）

1. コア: テストを先に書き、未実装（フィールド／コマンド無し）でコンパイルエラーになることを確認。実装後 green。
2. MCP: `tools_list_exposes_all_commands` に新ツール名が無いことで red。実装後 green。
3. フレーム API: テスト7件を先に書き、`run_frame_effect` 未実装でコンパイルエラー。実装後 green。
4. media 適用: テストを先に書き、`apply_script_effect_with_dirs` 未実装でコンパイルエラー（E0425）。実装後 green。書き出し側は `compose_export_frame_applies_script_effect` が「書き出しフレームが未加工のまま」で失敗する red を確認してから組み込み。
5. GUI セッション／パーサ: テストを先に書き、型未定義でコンパイルエラー。実装後 green。
6. レビュー指摘の回帰テスト（下記 round 1 / round 2）はいずれも修正前に失敗することを確認してから修正した。

## 単体テスト

- `cargo test -p video-editor`: 106 件 → 123 件（script +7、media +8、edit +2）。
- `crates/studio-core` command_tests: 64 件 → 67 件（+3）。`crates/studio-mcp`: 30 件 → 32 件（+2）。
- ワークスペース全体: 423 件すべて通過（並行レーンの追加分を含む）。
- `./scripts/verify.sh`: fmt / clippy / テストすべて OK。

## 実アプリ駆動（証拠）

- `docs/test-reports/evidence/issue-0081/drive.sh`（Xvfb :96 + lavapipe、実ディスプレイ／GPU での見え方は未検証）: DRIVE OK。
  - L キーで `invert.lua` を設定 → ダンプに `script_effect`、`script effect:` ログ、プレビュー画素照合 OK（`fx.png`）。
  - P キーで `level=3` → ダンプに `params.level = 3.0`。
  - `bad.lua` へ差し替え → `script error:` 表示・アプリ生存（`bad.png`）。
  - プロジェクトファイルに `script_effect`（bad.lua + params）が保存されていることを実物で確認。
- 証拠ファイル: `drive.sh`、`drive-app.txt`、`base.png`、`fx.png`、`bad.png`。
- 注記: GUI ドライブはレビュー round 1 のキャッシュキー修正より前に実施した。修正はキャッシュのキー付けのみで、GUI の操作・表示の振る舞いは変わらない（単体テストで担保）。

## 独立レビュー

- ラウンド1: CHANGES_REQUESTED。指摘2件、いずれも対応:
  1. （Major）キャッシュキーに時刻が無く、`clip.time_ms` 依存スクリプトの結果が stale になる。→ キーにクリップ相対時刻・クリップ名・長さを含めた。回帰テスト `script_effect_cache_key_includes_clip_time`。
  2. （Minor）書き出しがフレーム欠落時の黒フォールバックにもエフェクトを適用し、プレビューと食い違う。→ 黒フォールバック時は適用しないようガード。回帰テスト `compose_export_frame_skips_effect_on_black_fallback`。
- ラウンド2: CHANGES_REQUESTED。指摘1件、対応:
  1. （Major）非有限の数値パラメータ（NaN／±inf）が受理され、JSON で null 化してプロジェクト全体が再読込不能になる（GUI の P セッションから `level=nan` で到達可能）。→ コマンド層で有限値のみ許可（BPM・ピッチの前例どおり）。回帰テスト `set_clip_script_effect_rejects_non_finite_params`（拒否＋状態不変＋有限値の往復）。
- レビューは上限2ラウンドのため、round 2 指摘の修正は回帰テストと verify.sh で担保し、3ラウンド目は行っていない（修正内容はレビュアーの fix 方向そのまま）。

## 判断の記録

- エフェクトのモデルは単一属性（積み重ねなし）: #79 決定どおり。将来の拡張はモデル変更として別 PBI で扱う。
- パラメータ値の型解釈（数値 > 真偽 > 文字列）は実装レベルの判断として GUI パーサに置いた。スクリプト側は Lua の型としてそのまま受け取る。
- フレーム API は #81 の最小形（get／set／fill_rect）。図形・文字系の本格 API は AviUtl 互換層（#83）の `obj.*` で提供する。

## Definition of Done チェック

- [x] 受け入れ条件をすべて、書いてある確認方法で確かめた
- [x] TDD（red を記録）
- [x] `cargo test` が 0 で終わった（ワークスペース 423 件）
- [x] 画面で観測できる受け入れ条件を実アプリ駆動＋スクリーンショットで確認した
- [x] 状態を変える受け入れ条件をダンプ（GUI ログ／MCP）で確認した
- [x] 独立レビューを受けた（2ラウンド、指摘はすべて対応）
