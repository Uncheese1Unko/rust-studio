# AviUtl（ExEdit）Lua スクリプト互換の範囲

Issue #83 の互換プロファイル（`crates/studio-scripts/src/aviutl.rs`）が
対応する範囲と、意図的に外した範囲、ExEdit と異なる振る舞いの規則を
ここに集約する。方式の決定は ADR-0019（方式 A: ExEdit Lua スクリプト
エフェクト互換に絞る）、形式の調査根拠は
`docs/test-reports/issue-0079-aviutl-compat-investigation.md`。

未対応の API・効果を呼ぶと、黙って壊さず **明示的な実行時エラー**
になり、アプリ側ではスクリプトエラーとして表示される（エフェクト
なしで表示は続く）。

## 取り込み方

- `.anm` / `.obj` ファイルをスクリプトフォルダへ置くと一覧に出る
  （#82 の管理対象。構文が壊れたものはエラーとして区別される）。
- クリップへの適用は `Clip.script_effect` の参照で行う。セクションを
  指定する場合は `ファイル.anm@セクション名` と書く（区切りは
  拡張子の直後の `@`。ファイル名自体が `@` で始まってもよい）。
- パラメータは既存のパラメータ編集（P キー、`key=value`）で変える。
  キーは `track0`〜`track3`、`check0`、`--dialog:` の変数名、`color`。

## ファイル形式の扱い

- 文字コードは SJIS 前提でバイト列のまま処理する。UTF-8 への変換は
  セクション名・ラベル・描画文字列などのホスト境界でのみ行う。
- セクションは `@名前` 行で分割する。ディレクティブは
  `--track0:`〜`--track3:`、`--check0:`、`--dialog:`、`--color:`、
  `--file:`、`--param:` を解釈する（`--param:` は本体の前に
  コードとして実行する）。
- `--dialog:` の種類接尾辞（`/chk`・`/col`・`/fig`）は lua.txt
  の実形式どおり**表示名の最後**に置く。文字列の初期値は引用符
  （`"..."`）で囲まれた形を受け、引用符を外して保持する
  （例: `--dialog:サイズ,size=100;図形/fig,fig="円"`）。
- ファイル名は UTF-8 前提（OS のファイル名が SJIS バイト列のものは
  そのままでは扱えない）。

## バッファモデルと描画規則

- バッファは「オブジェクト（obj）」「画面（frm）」「一時（tmp）」
  「キャッシュ（cache:名前）」の4種。`.anm` ではオブジェクトは
  入力フレームのコピーから、`.obj` では透明から始まる。
- `obj.draw()` 等で画面へ描かれた場合はその結果がフレームになる。
  一度も描かれなかった場合は、オブジェクトバッファをそのまま
  画面へ重ねた結果になる（ピクセル操作だけのスクリプトが効く
  ための規則）。
- 座標は ExEdit と同じく画面中心が原点。サンプリングは最近傍。
- `obj.frame` / `obj.totalframe` は 30fps 前提の換算（時刻
  `obj.time` は正確）。プレビューと書き出しで同じ規則を使う。

## 対応する API

- 読み込み: `obj.load` の `figure`（四角形・円・三角形・五角形・
  六角形・星形。第4引数 `line` 指定時は輪郭のみ描く）、`text`
  （色は `setfont` の col1。第3・第4引数は speed（文字/秒）と
  time で、経過時間までに表示される文字数だけ描くタイプライタ
  表示）、`image`（PNG 等、スクリプトフォルダ相対。フォルダ外
  への遡り・絶対パスは require と同じく明示エラー）、`object`、
  `before`（入力フレームで代用）、`framebuffer`、`tempbuffer`
- 描画: `obj.draw`（lua.txt の引数順
  `[ox, oy, oz, zoom, alpha, rx, ry, rz]`。合成モードは引数では
  なく `setoption("blend")` の状態を使う。oz・rx・ry は 2D
  合成のため無視）、`obj.drawpoly`（射影変換。引数は
  `(x,y,z)×4` の後に UV `(u,v)×4` と alpha が任意で続く）、
  `obj.mes`、`obj.setfont`
  （引数は ExEdit の署名どおり: name, size, type（装飾 0〜6）,
  col1（文字色）, col2（影・縁の色）, bold, italic, 文字間隔,
  行間隔。フォント名は記録のみで、グリフはホストのシステム
  フォントで描く。システムフォントの探索は Linux の候補パス
  （Noto CJK / DejaVu）のみで、見つからない環境ではテキスト
  描画が明示エラーになる。装飾は近似描画: 影付きは col2 の
  オフセット描画、縁取りは col2 の周囲描画。太字は二重描きの
  近似、斜体は未対応で無視する）
- バッファ操作: `obj.copybuffer`（obj / tmp / frm / cache:名前 /
  image:パス。成功時は `true` を返す）、`obj.getpixel` /
  `obj.putpixel` / `obj.copypixel`（戻り値の形は lua.txt
  どおり: 引数なしで `(幅, 高さ)`、col 形式は α が 0.0〜1.0 の
  実数、rgb 形式は α が 0〜255）、`obj.getpixeldata` /
  `obj.putpixeldata`
  （ピクセルデータは RGBA バイト文字列として受け渡す。ポインタ
  経由の直接アクセスは LuaJIT/FFI 前提のため対象外＝ADR-0019 の
  上限どおり）
- ピクセル形式: `obj.pixeloption(name, value)` の
  `("type", "col"/"rgb")`、`("get", "obj"/"frm")`、
  `("put", "obj"/"frm")`、`("blend", nil/数値/名前)`
- 効果: `obj.effect` の **色調補正・単色化・ぼかし・色反転**
  （未知の効果名は明示エラー。既知の効果の未知パラメータは無視）
- 合成モード: 名前と lua.txt の数値 0〜9（通常・加算・減算・
  乗算・スクリーン・オーバーレイ・比較（明）・比較（暗）・
  輝度・色差）。輝度・色差は HSV 空間での近似（輝度 = dst の
  色相・彩度 + src の明度、色差 = src の色相・彩度 + dst の
  明度）
- オプション・情報: `obj.setoption` の `blend` / `drawtarget`
  （正式名は `"tempbuffer"` / `"framebuffer"`、`"tmp"` /
  `"frm"` / `"obj"` も受ける。tempbuffer は w,h 指定で透明に
  初期化。`antialias` / `culling` は受け入れるが効果を持た
  ない）、`obj.getoption` の `script_name` / `section_num`
  （中間点を持たないため常に 1）/ `track_mode` / `gui`
  （真偽値 false）/ `blend` / `drawtarget`（正式名を返す）、
  `obj.getinfo` の `script_path`（スクリプト**フォルダ**の
  パス）/ `saving`（常に false）/ `image_max`
  （`(4096, 4096)` の2値）
- その他: `obj.rand`（決定的な乱数）、グローバル `RGB` / `HSV`
  （3引数の順変換のほか、1引数の逆変換と6引数のグラデーション
  形式＝オブジェクトの時間進行で補間に対応）、
  `require`（スクリプトフォルダ内の `<名前>.lua` を読み込む。
  フォルダ外へのパス遡りは明示エラー）

## 未対応（明示エラーになる）

- `obj.filter`、`obj.getvalue`、`obj.getaudio`、`obj.getpoint`、
  `obj.setanchor`、`obj.interpolation`
- `obj.load` の `movie` / `layer`
- `obj.effect` の初期セット以外の効果、`obj.effect()` の引数なし
  呼び出し（後続エフェクトの実行）
- `obj.pixeloption("type", "yc")`（YC 形式）、
  `obj.getpixel(x, y, "yc")`
- LuaJIT / FFI、`obj.getpixeldata` のポインタ形式
- `.scn` / `.cam` / `.tra`（ADR-0019 で対象外）
- トラックバーのアニメーション移動（中間点・移動量計算）:
  `obj.track0` 等は現在時刻の値のみを返す

## 検証について

- 検証は配布形式に倣って自作した SJIS の `.anm` サンプル
  （図形系・テキスト系）と単体テスト・実アプリ駆動・mp4 書き出しの
  ピクセル確認で行った（`docs/test-reports/issue-0083-aviutl-anm-import.md`）。
- 第三者の配布スクリプトそのものの実行照合は行っていない
  （理由は同レポートに明記）。
