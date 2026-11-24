# 試験レポート: #83 AviUtl 形式スクリプト（.anm 等）を取り込んで使える

対象 PBI: #83「AviUtl 形式スクリプト（.anm 等）を取り込んで使える」
実施日: 2026-11-24
実装コミット: 本レポートと同梱（Refs #83）
ベース: `d13cee5`

## 引き継ぎと精査の経緯（正直な記録）

本 PBI は前任エージェントが未コミットの作業ツリー変更だけを残して
最終報告なしに終了したため、後任が差分を精査して仕上げた。
精査時点の状態は次のとおりだった。

- **実装済みだったもの**: 互換プロファイル本体
  （`crates/studio-scripts/src/aviutl.rs`、約2,200行。SJIS の
  まま処理する `.anm`/`.obj` の解析、パラメータ指令の解釈、
  `obj.*` 第1段 API、名前付き効果の再実装、未対応の明示エラー）、
  カタログへの `.anm`/`.obj` 統合（#82 の一覧、セクション参照の
  解決）、適用経路（`media.rs`、プレビュー／書き出し共通、
  キャッシュキーへの参照文字列とソースハッシュの追加）、
  対応範囲ドキュメント `docs/aviutl-compat.md`、駆動スクリプト
  `evidence/issue-0083/drive.sh`、単体テスト21件。
- **不足していたもの**: (1) `./scripts/verify.sh` が clippy で
  落ちる状態だった（aviutl.rs の lint 違反6件）。(2) 駆動
  スクリプトは未実行で、テキストセクションの実アプリ確認が
  なかった。(3) 試験レポートが無く、`docs/aviutl-compat.md`
  から参照だけが存在した。(4) 独立レビュー未実施、コミット・
  close 未実施。
- **後任が補ったもの**: clippy 修正（意味を変えない機械的置換）、
  駆動の実行とテキストセクション確認フェーズの追加、駆動で
  見つけた不具合1件の修正（下記）、レビュー round 1・round 2
  指摘への対応（下記）、本レポートと ADR-0021。

前任分のテストについては、red（失敗）の記録が残っていない。
後任が追加したテストについては red を確認してから実装した
（「TDD の記録」参照）。

## 実装内容

- **互換プロファイル（`crates/studio-scripts/src/aviutl.rs`）**:
  #80 の基盤の上に「AviUtl 互換プロファイル」として層を分けた
  （独自 API とは混ぜない。配置先は ADR-0020 により共有クレート
  で、イシューの「触るファイル」（apps/video-editor/src に新規
  モジュール想定）とは異なる。レビュー round 1 の観察事項として
  指摘され、ADR-0020 とイシュー自身の実装方針（基盤の上に層を
  分ける）に整合するため、この配置を正とした）。
  - ファイルは SJIS バイト列のまま処理し、`@名前` セクション
    分割と指令（`--track0..3`、`--check0`、`--dialog:`、
    `--color:`、`--file:`、`--param:`）を解釈する。UTF-8 変換は
    セクション名・ラベル等のホスト境界でのみ行う。
  - バッファは obj／frm／tmp／cache:名前 の4種。`.anm` は入力
    フレームのコピーから、`.obj` は透明から始める。一度も画面へ
    描かれなかった場合はオブジェクトバッファをそのまま重ねる
    （ピクセル操作だけのスクリプトが効くための規則）。
  - API 第1段（ADR-0019）: `obj.load`（figure／text／image／
    object／before／framebuffer／tempbuffer）、`obj.draw`／
    `drawpoly`（射影変換）、`obj.mes`／`setfont`、`obj.copybuffer`、
    `obj.getpixel`／`putpixel`／`copypixel`、`obj.getpixeldata`／
    `putpixeldata`（RGBA バイト列として受け渡し。ポインタ形式は
    LuaJIT/FFI 前提のため ADR-0019 の上限どおり対象外）、
    `obj.pixeloption`（2引数形式: type／get／put／blend）、
    `obj.setoption`／`getoption`／`getinfo` の部分集合、
    `obj.rand`、`RGB`／`HSV`（逆変換・グラデーション形式を
    含む）、`require`（スクリプトフォルダ内のみ）。
  - `obj.effect` は再実装した名前付き効果（色調補正・単色化・
    ぼかし・色反転）に対してだけ動く。初期セットの判断は
    **ADR-0021** に記録した。未知の効果名・未実装 API
    （`obj.filter`、`obj.getvalue`、`obj.getaudio` 等）は黙って
    壊さず明示的な実行時エラーにし、アプリ側ではスクリプト
    エラーとして表示される（表示はエフェクトなしで続く）。
  - 対応／未対応の一覧は `docs/aviutl-compat.md` に集約した。
- **カタログ（`catalog.rs`）**: 拡張子に `anm`／`obj` を追加し、
  #82 の一覧で同じく扱う。構文検証は SJIS のままセクション
  単位で行い、壊れたものは Error として区別する。参照
  `ファイル.anm@セクション名` はファイル部で状態を判定する。
- **適用経路（`media.rs`）**: 参照をファイル部とセクションに
  分解し、拡張子で互換プロファイル／従来 Lua を振り分ける。
  プレビューと書き出しは従来どおり単一経路（ADR-0015 の原則）
  で、両方に同じ結果が反映される。キャッシュキーに参照文字列
  （セクション指定を含む）とソースのハッシュを入れた（同一
  ファイルの別セクションが混ざらないため）。

## 駆動で見つけた不具合と修正

- **スクリプト名入力で `@` が入力できない**（駆動で検出）:
  L キーのスクリプト名セッションが受け付け文字から `@` を
  落としており、GUI からはセクション参照を一切入力できな
  かった（入力すると `sample.anmx` のように潰れる）。
  `ScriptNameSession` の追加受け付け文字に `@` を加えた。
  回帰テスト `script_name_session_accepts_section_reference_at_sign`
  を先に書いて red（`fx.anmsec-1` になる）を確認してから修正。

## コードレビュー

- **round 1: CHANGES_REQUESTED**（独立 code-reviewer、イシュー
  番号と diff 範囲のみを渡した）
  1. （Major）`obj.setfont` の引数対応付けが ExEdit の署名と
     違う。ExEdit は `(name, size, type, col1, col2, bold,
     italic, charspacing, linespacing)` で col1 が文字色。
     旧実装は arg2／arg3／arg4 を太字／斜体／色としており、
     実在形式の `setfont("MS Gothic", 24, 0, 0xff0000)` が
     白で描かれる静かな非互換だった。同梱サンプルも誤った
     対応付けで書かれており検出できていなかった。
     → 署名どおりに対応付け直し、装飾タイプ（type）は col2 を
     使った影・縁取りの近似描画を実装。サンプル（単体テスト・
     駆動）を正しい形式に直し、回帰テスト
     `setfont_col1_is_the_text_color` /
     `setfont_shadow_type_draws_col2_shadow`（いずれも red を
     確認してから修正）を追加した。
  2. （Minor）新規ドキュメントに `docs/log.md` の記録が無い、
     compat ドキュメントが参照する試験レポートが存在しない。
     → 本レポートの作成と log.md への記録で解消。
  3. （Minor）`obj.effect` 初期セットの判断が ADR に無い。
     → ADR-0021 を作成した。
  4. （Minor）互換プロファイルの `require` が `../` でスクリプト
     フォルダ外を読める。→ 候補パスの canonicalize でフォルダ
     内包含を必須にし、外は明示エラーにした。回帰テスト
     `require_cannot_escape_scripts_dir`（red 確認後に修正）と
     `require_loads_module_inside_scripts_dir` を追加。
  5. （観察）触るファイルの配置がイシュー本文と異なる
     （studio-scripts 配置）。→ ADR-0020 と実装方針に整合する
     ため変更不要。本レポートに明記した（上記「実装内容」）。
- **round 2: CHANGES_REQUESTED**（新しい独立レビュアー）。
  round 1 と同種の「lua.txt の署名との対応付け違い」がさらに
  6件 Major・2件 Minor で見つかった。いずれもレビュアーの
  転記と lua.txt の記載を後任が照合して正しいことを確認し、
  範囲内（受け入れ条件の核＝実在形式のスクリプトがそのまま
  動くこと）として全件対応した。
  1. （Major）`obj.draw` の引数対応付け。実形式は
     `[ox, oy, oz, zoom, alpha, rx, ry, rz]` で合成モード
     引数は無い。旧実装は `(x, y, zoom, alpha, blend)` と
     読んでおり、`obj.draw(2, 10, 0)` が zoom=0 で無描画に
     なる等の静かな非互換があった。→ 署名どおりに是正し、
     blend は setoption の状態を使う形にした。
  2. （Major）`obj.drawpoly` の引数対応付け。実形式は
     `(x,y,z)×4` の後に UV `(u,v)×4` と alpha。旧実装は
     `(x,y)×4 + alpha + blend` と読んでおり、lua.txt の
     標準例が歪んだ四角形になっていた。→ 是正し、UV 指定も
     実装した（composite_poly に src 四隅を渡す）。
  3. （Major）`--dialog:` の接尾辞の位置。実形式では表示名の
     最後に付く（`--dialog:色/col,col=0xffffff` 等）。旧実装は
     変数名側から読んでおり、`/fig` の初期値の引用符も外れて
     いなかった（未知図形エラーになる）。→ 是正。
  4. （Major）合成モードの数値対応。実形式は 5=オーバーレイ、
     6=比較（明）、7=比較（暗）、8=輝度、9=色差。旧実装は
     5/6 がずれ、7〜9 はエラーだった。drawtarget の正式名
     （`"tempbuffer"` / `"framebuffer"`）も未対応だった。
     → 0〜9 全てを実装（輝度・色差は HSV での近似と明記）、
     正式名＋w,h での tempbuffer 初期化を実装。
  5. （Major）`obj.load("text", text, speed, time)` の第3引数
     は色ではなく表示速度（文字/秒）。旧実装は色として扱って
     いた。→ 色の上書きを廃し、speed×time 文字までの
     タイプライタ切り取りを実装。
  6. （Major）ピクセル API 群の戻り値・引数形式。
     `obj.getpixel` は col 形式で `(col, α)`（α は 0.0〜1.0）、
     rgb 形式で `(r, g, b, a)`、引数なしで `(幅, 高さ)` を
     返す。`obj.putpixel` も同形式で α を取る。
     `obj.pixeloption` は `(name, value)` の2引数形式で
     `"type"` / `"get"` / `"put"` / `"blend"` を切り替える。
     旧実装はいずれも異なる形だった。→ lua.txt どおりに
     作り替えた（get/put 先の frm 切替と putpixel の blend
     合成を含む）。
  7. （Minor）`getoption("section_num")` は区間数（中間点+1）
     で常に 1、`getoption("gui")` は真偽値、`getinfo` の
     `script_path` はフォルダのパス、`image_max` は2値。
     → すべて是正（Host の不要フィールドも削除）。
  8. （Minor）`obj.load("figure", ...)` の第4引数 `line`
     （輪郭幅）が無視されていた、`RGB` / `HSV` の1引数
     逆変換と6引数グラデーション形式が未対応、`obj.copybuffer`
     の戻り値（true）が無かった。→ いずれも実装した。
  回帰テスト9件（`draw_uses_exedit_argument_order` ほか）を
  先に書いて red を確認し、既存の
  `parse_sections_and_directives` も実形式に合わせて更新した。
  対応後 `./scripts/verify.sh` 緑（475件）。
- **round 3: CHANGES_REQUESTED**（新しい独立レビュアー）。
  受け入れ条件は全項目充足と確認されたが、Minor 2件の指摘が
  あったため差し戻しになった。
  1. （Minor）`obj.load("image")` と `copybuffer` の `image:`
     には round 1 で是正した require と同じフォルダ内包含
     チェックが無く、`../` や絶対パスでフォルダ外の画像を
     読めた（compat ドキュメントの「スクリプトフォルダ相対」
     の記述とも不一致）。→ 共通ヘルパー
     `resolve_inside_scripts_dir`（canonicalize＋包含必須、
     外は「フォルダ外」明示エラー）を両経路に適用した。
     回帰テスト `load_image_cannot_escape_scripts_dir` /
     `copybuffer_image_cannot_escape_scripts_dir`（いずれも
     red を確認してから修正）を追加。
  2. （Minor）compat ドキュメントと aviutl.rs 冒頭が参照する
     #79 調査レポートのファイル名が実在しないものだった
     （`issue-0079-aviutl-investigation.md` → 実ファイルは
     `issue-0079-aviutl-compat-investigation.md`）。→ 2箇所
     とも是正。
  あわせて観察事項（システムフォント探索が Linux 候補パスの
  みで、無い環境ではテキスト描画が明示エラーになる制限が
  compat ドキュメントに未記載）も、ドキュメントへ明記して
  対応した。
- **round 4: APPROVE**（新しい独立レビュアー。なお最初の
  round 4 レビュアーは報告なしに終了したため、作業ツリーが
  無変更であることを確認のうえ別のレビュアーで再実施した）。
  round 3 指摘2件への対応（画像読み込みの包含チェックと
  回帰テスト、参照ファイル名の是正）と観察事項への
  ドキュメント対応が正しく行われていること、round 1・2 の
  是正に巻き戻りが無いこと、受け入れ条件の全項目充足、
  `./scripts/verify.sh` 緑（477件）をレビュアー自身の実行で
  確認。非ブロッキングの観察として「require は新設ヘルパー
  ではなく従来のインライン包含チェックのまま（意味論は同等）」
  が記録された。

## 受け入れ条件の確認

- [x] 範囲内の .anm サンプル（図形系・テキスト系を各1つ）が
  取り込めて、クリップに適用してプレビューに反映される
  — media テスト `anm_script_effect_draws_figure_on_frame`、
  aviutl テスト `text_sample_renders_glyph_pixels`、実アプリ
  駆動（図形: 適用後フレーム中心が赤・赤ピクセル18,696件。
  テキスト: 緑グリフピクセル7,957件。証拠 `figure.png` /
  `text.png`）
- [x] パラメータ定義が解釈され、GUI で値を変えると描画が
  変わる — `parse_sections_and_directives`、
  `track_param_override_changes_rendering`、
  `check0_and_dialog_globals_drive_script`、駆動（P キーで
  `track0=40` → ダンプに反映され赤円が縮小。証拠
  `figure-small.png`）
- [x] 範囲外 API のスクリプトは未対応がエラーとして観測
  できる — `unsupported_api_and_effect_error_explicitly`、
  media テスト `anm_unsupported_api_surfaces_as_error`、
  駆動（存在しないセクション指定で「見つかりません」の明示
  エラー、アプリは生存。証拠 `bad-section.png`）
- [x] 書き出し（mp4）でもプレビューと同じ結果になる
  — media テスト `anm_export_frame_matches_preview_pixels`
  （書き出しフレームとプレビューの画素一致を assert）、
  駆動（`--export-mp4` の書き出しフレーム 1280×720 に赤円
  137,950ピクセル。証拠 `export-frame.png`）

## TDD の記録（red）

後任が追加したテストについてのみ、red を確認した記録が残る
（前任分は引き継ぎ時に記録が無かった。「引き継ぎと精査の
経緯」参照）。

1. `@` 入力: `script_name_session_accepts_section_reference_at_sign`
   を先に書き、`fx.anmsec-1`（`@` が落ちる）で red。修正後 green。
2. setfont: `setfont_col1_is_the_text_color` /
   `setfont_shadow_type_draws_col2_shadow` を先に書き、旧対応
   付け（白で描画／影なし）で red。署名どおりの対応付けと
   装飾描画の実装後 green。
3. require: `require_cannot_escape_scripts_dir` を先に書き、
   フォルダ外ファイルが読めてしまう（Ok が返る）ことで red。
   包含チェックの実装後 green。
4. round 2 の対応付け是正: 回帰テスト9件
   （`draw_uses_exedit_argument_order`、
   `drawpoly_uses_exedit_argument_order`、
   `blend_index_five_is_overlay_and_drawtarget_canonical_names`、
   `load_text_speed_is_not_a_color`、`pixel_apis_follow_lua_txt`、
   `getoption_getinfo_follow_lua_txt`、
   `figure_line_draws_outline_only`、`copybuffer_returns_true`、
   `rgb_hsv_reverse_and_gradient_forms`）と実形式に更新した
   `parse_sections_and_directives` を先に書き、10件の red を
   確認してから実装した。なお `load_text_speed_is_not_a_color`
   は当初 speed を整数リテラルで書いたところ旧実装でも green
   になってしまった（mlua では整数リテラルが Integer として
   渡り、旧実装の色上書きが Number にしか反応しなかったため）。
   小数リテラル（10.5）に直して red を確認した経緯も含めて
   記録する。

5. round 3 の画像境界: `load_image_cannot_escape_scripts_dir` /
   `copybuffer_image_cannot_escape_scripts_dir` を先に書き、
   フォルダ外の PNG が読めてしまう（実行が成功する）ことで
   red。包含チェックの実装後 green。

## 単体テスト

- `crates/studio-scripts`: 68 件（aviutl 30、catalog 含む。
  レビュー round 1 の回帰テスト4件、round 2 の回帰テスト
  9件、round 3 の回帰テスト2件を含む）
- `apps/video-editor`: 106 件（media の .anm 適用・セクション
  区別・未対応エラー・書き出し一致、`@` 回帰テストを含む）
- ワークスペース全体: **477 件**すべて通過（#82 完了時点の
  440 件から +37）。`./scripts/verify.sh`: fmt / clippy /
  テストすべて OK。

## 実アプリ駆動（証拠）

- スクリプト: `docs/test-reports/evidence/issue-0083/drive.sh`
  （Xvfb :96 + lavapipe。**実ディスプレイ／GPU での見え方は
  未検証**）。結果は全フェーズ OK（DRIVE OK）:
  起動時ダンプで sample.anm が ready・参照 ready、図形
  セクションの赤円、P キーでのパラメータ変更と円の縮小、
  未知セクションの明示エラーとアプリ生存、mp4 書き出し
  フレームの赤円、テキストセクションの緑グリフ。
- スクリーンショット: 同ディレクトリの `figure.png` /
  `figure-small.png` / `bad-section.png` / `text.png` /
  `export-frame.png`。

## 検証の留保

- **第三者の配布スクリプトそのものの実行照合は行っていない。**
  配布スクリプトは作者ごとにライセンスが異なり、本リポジトリ
  へ同梱できないものを含むため、検証は配布形式に倣って自作
  した SJIS サンプル（図形系・テキスト系・未対応 API）で行った。
  実在スクリプトでの照合は、利用者が手元のスクリプトで確認
  する事項として残る（未知 API・未知効果は明示エラーになる
  ため、非互換は静かにはならない）。
- `obj.frame` / `obj.totalframe` は 30fps 前提の換算（時刻
  `obj.time` は正確）。プレビューと書き出しで同じ規則を使う。
- フォント名は記録のみで、グリフはホストのシステムフォント
  で描く。装飾（影・縁取り）と太字は近似描画、斜体は未対応
  （`docs/aviutl-compat.md` に明記）。
