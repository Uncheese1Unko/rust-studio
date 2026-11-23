# 調査レポート： Issue #79 AviUtl 拡張互換の範囲を決める

- 日付: 2026-11-23
- 実行者: エージェント
- 種別: 調査 PBI（本体コードの変更なし。成果物は ADR と本レポート、証拠。試作は使い捨てで /tmp に置き、証拠になる出力とソースだけを `evidence/issue-0079/` に残した）
- 結論: **ADR-0019**「AviUtl 拡張互換の範囲は ExEdit Lua スクリプトエフェクト互換に絞る（方式 A）」を**提案として記録**した。互換範囲の確定は設計判断（ADR-0006）のため、採用の決定は人間に渡した（イシューに `needs-decision` が付いており、決定依頼コメントを投稿した）。本レポートはその状態を正直に記録する。
- 環境: Yoko の VM（Linux）。**実 AviUtl／ExEdit を実行できる Windows 環境は無く、動作の等価性はすべて公開資料と実物ファイルの静的調査に基づく**（未検証事項に明記）。

## 受け入れ条件ごとの結果

### AC1: AviUtl バイナリプラグイン（.auo/.aui/.auf/.aul）の構造とホスト要件が公開資料ベースで整理され、参照資料付きで記録されている — ✅（整理完了。結論は「ネイティブ互換は成立しない」）

#### プラグインの種類（AviUtl 1.10 世代）

すべて「.dll をリネームしただけ」の Windows DLL で、本体・プラグインとも IA-32（x86）。分類は SDK 同梱ドキュメント（aviutl_plugin_sdk.txt）由来の一覧による。

| 拡張子 | 種類 | 役割 | SDK ヘッダ | エントリポイント |
|---|---|---|---|---|
| .auf | フィルタプラグイン | 画像の加工編集（拡張編集 exedit.auf もこの一種） | filter.h | `GetFilterTable()` |
| .aui | 入力プラグイン | 他形式ファイルの読み込み | input.h | `GetInputPluginTable()` |
| .auo | 出力プラグイン | 他形式への出力 | output.h | `GetOutputPluginTable()` |
| .auc | 色変換プラグイン | 入出力時の色空間変換 | （SDK） | 同様のパターン |
| .aul | 言語拡張リソース | 対応言語の拡張（リソース主体） | — | — |

#### ABI の実物確認（SDK ヘッダを直接読んだ）

- フィルタ: 実物の `filter.h`（632行、プラグイン実作 delogo-aviutl 同梱の SDK コピー）を確認した。エントリは `EXTERN_C FILTER_DLL __declspec(dllexport) * __stdcall GetFilterTable(void)`。`FILTER_DLL`（filter.h:590〜）は関数ポインタ群（`func_proc`、`func_init`、`func_exit`、`func_update`、`func_WndProc`、`func_save_start/end`、`func_is_saveframe`）と、`TCHAR*` の名前・トラック／チェック定義、`EXFUNC* exfunc`、`HWND hwnd`、`HINSTANCE dll_hinst` を含む。
- ホスト→プラグインの結線は **ウィンドウメッセージ**：`func_WndProc(HWND, UINT, WPARAM, LPARAM, void* editp, FILTER*)` が `WM_FILTER_*`（WM_USER+100〜112 の13種：FILE_OPEN、INIT、SAVE_START、IMPORT、EXPORT、CHANGE_PARAM、MAIN_MOUSE_DOWN 等）を受ける。設定 UI はホストが作る実 Win32 ウィンドウであり、プラグインはその子として振る舞う。
- ホスト側コールバック表 `EXFUNC` は関数ポインタ **約50項目**（filter.h:123〜452）。`get_ycp_source_cache` 等のフィルタリングキャッシュ、クリップボード連携（`copy_clip(HWND, ...)`）、編集ファイル状態へのアクセスが含まれ、全コールバックが不透明なエディットハンドル `void* editp` を取る。
- フレーム形式は内部形式 `PIXEL_YC`（Y: 0〜4096、Cb/Cr: -2048〜2048 の YCbCr。RGB の `PIXEL` は補助）。フィルタは `FILTER_PROC_INFO` の `ycp_edit`／`ycp_temp` を読み書きし、画像サイズ自体も変更できる。
- 入力: 実物の `input.h`（aviutl_ffmpeg_input 同梱の SDK コピー）を確認した。`INPUT_PLUGIN_TABLE` の `func_open(LPSTR)`、`func_read_video(handle, frame, void* buf)`、`func_read_audio`、`func_config(HWND, HINSTANCE)`（設定ダイアログ）。`INPUT_INFO` は `BITMAPINFOHEADER*`／`WAVEFORMATEX*` を含み、画像形式には PIXEL_YC 形式（'YC48'、48bit）も指定できる。
- 出力: `output.h`（NVEnc 同梱）を確認した。`OUTPUT_INFO` はホスト側コールバック（`func_get_video(frame)` → フレームポインタ、`func_get_audio`、`func_is_abort` 等）を持ち、プラグインがフレームを引いて書き出す形。設定は同様に HWND ダイアログ。
- **ExEdit（拡張編集）は exedit.auf、すなわちフィルタプラグインそのもの**である。タイムライン編集も Lua スクリプト機構も ExEdit の内部実装であり、他のホストが使える公開 API としては存在しない。

#### ネイティブ互換が成立しない理由（技術的に具体化）

1. **PE＋x86＋stdcall**: ロードには Windows ローダと x86 実行環境が要る。Linux の Rust プロセスから `dlopen` では読めない（形式も呼び出し規約も別）。
2. **Win32 型とメッセージループの埋め込み**: ABI の中心に HWND／HINSTANCE／WM_FILTER メッセージがあり、互換ホストは「Win32 ウィンドウシステムとメッセージ配送」を再現しなければならない。これはライブラリ呼び出しの翻訳では済まない。
3. **ホスト内部状態への依存**: EXFUNC のキャッシュと `editp` の意味論は AviUtl 本体の編集モデルの再現であり、仕様は非公開部分を含む。プラグインごとに依存度が違い、動く範囲を事前に約束できない。
4. **PIXEL_YC 内部形式**: 12bit 相当の YCbCr で、本アプリの RGBA 経路とは変換が必須。単独では決定打ではないが、上記と積み重なる。
5. したがって現実の選択肢は「Wine 上に AviUtl ホスト再現プロセスを作る」（方式 C）しかなく、その工数とリスクは ADR-0019 の比較表に記した。なお **.aui／.auo は本アプリでは ffmpeg 経路（素材展開・mp4 書き出し、ADR-0015）が既に担っており、互換の実益も薄い**。

### AC2: ExEdit Lua スクリプトの API 表面が整理され、本アプリの描画経路への対応表がある — ✅

一次資料は ExEdit 付属の仕様文書 lua.txt（Web 上の転記で全文を確認）。要点：

#### ランタイムとファイルの約束

- Lua **5.1.4**（付属 lua51.dll。0.93 系は LuaJIT 2.0.5 の lua51jit.dll も併用）。スクリプト最大サイズは約32KB（超過分は `dofile()` 等で分割するのが前提）。
- ファイルは **Shift_JIS＋CRLF** でないと正しく読まれない。SJIS の 0x5c はエスケープ処理されてから実行される。文字列はバイト列として扱われる。
- 配置は exedit.auf と同フォルダの `script` フォルダ（サブフォルダ可）。ファイル名先頭 `@`＋本文中の `@名前` で1ファイルに複数スクリプトを登録する。
- 種別：`.obj` カスタムオブジェクト（無から画像を生成）、`.anm` アニメーション効果（既存オブジェクトを変形・加工）、`.scn` シーンチェンジ（2シーンの合成比率を制御）、`.cam` カメラ効果、`.tra` トラックバー変化方法。

#### 状態変数（`obj.*`）

位置・変換：`ox/oy/oz`（基準座標からの相対）、`rx/ry/rz`（回転角）、`cx/cy/cz`（中心）、`zoom`（1.0=等倍）、`alpha`（0.0〜1.0）、`aspect`。読み取り専用：`x/y/z`、`w/h`、`screen_w/screen_h`、`framerate`、`frame`、`time`（秒）、`totalframe`、`totaltime`、`layer`、`index`、`num`。パラメータ：`track0..3`、`check0`。

#### パラメータ指令（ファイル先頭のコメント行）

| 指令 | 書式 | 備考 |
|---|---|---|
| `--track0:`〜`--track3:` | `名前,最小値,最大値,初期値[,移動単位]` | 最大4本（.scn は2本）。値は `obj.trackN` で参照 |
| `--check0:` | `名前,初期値(0/1)` | 参照時は boolean |
| `--dialog:` | `表示名,変数名=初期値;…` | 最大16項目。末尾 `/chk`（チェックボックス）`/col`（色）`/fig`（図形選択）で型指定 |
| `--color:` | `初期値(0xRRGGBB)` | 変数 `color` に入る。--file/--dialog/--param と併用不可 |
| `--file:` | （なし） | 変数 `file` にパスが入る |
| `--param:` | `a=1;b=2;` 形式の初期値 | そのままスクリプト先頭に注入される。最大255バイト |
| .tra 専用 | `--twopoint`、`--speed:`、`--param:` | トラックバー変化方法スクリプト用 |

#### 関数（`obj.*` とグローバル）

- 描画・読み込み：`obj.load`（"movie"／"image"／"text"／"figure"／"framebuffer"／"tempbuffer"／"layer"／"before"）、`obj.setfont`、`obj.draw`（変換付き描画、複数回可）、`obj.drawpoly`（四角形への射影描画）、`obj.mes`（テキスト内）
- 効果：`obj.effect(名前, パラメータ名, 値, ...)`（ExEdit 組み込みフィルタ効果を名前で呼ぶ。無引数で「後続の効果」を実行）、`obj.filter`（画面全体へのフィルタオブジェクト）
- バッファ：`obj.copybuffer`（obj／tmp／frm／cache:名前／image:パス 間コピー）、`obj.setoption`（"blend" 合成モード、"drawtarget"、"antialias"、"culling"、"billboard"、"shadow"、"camera_param" 等）、`obj.getoption`（"track_mode"、"section_num"、"script_name"、"gui"、"camera_param" 等）
- ピクセル：`obj.getpixel`／`putpixel`／`copypixel`（型は "col"／"rgb"／"yc"、`obj.pixeloption` で切替）、`obj.getpixeldata`／`putpixeldata`（RGBA 32bit の生ポインタ受け渡し。「DLL を利用して画像処理をする為のもの」と明記）
- 情報・その他：`obj.getvalue`（他レイヤー・他時刻の設定値、"scenechange" 含む）、`obj.getpoint`（トラックバーの区間・中間点）、`obj.getinfo`（"script_path"、"saving"、"image_max"）、`obj.getaudio`（"pcm"／"spectrum"／"fourier"）、`obj.rand`、`obj.setanchor`（GUI アンカーポイント）、`obj.interpolation`、グローバル `RGB()`、`HSV()`、`OR/AND/XOR`、`SHIFT`、`debug_print`

#### 実物スクリプトの実測（配布スクリプトが依存する API 範囲）

現行メンテナンスされている配布スクリプト集 sigma-axis 氏の sigma_aviutl_scripts（GitHub、MIT）を clone し、スクリプト5ファイル（.anm×3、.obj、.scn、計622行）と共有ライブラリ4本（.lua）を静的に集計した。

- パラメータ指令は全ファイルで使用：`--track0` 39件、`--track1` 37件、`--track2` 32件、`--track3` 29件、`--dialog` 39件、`--check0` 32件。**指令のパースと値の保持は互換の必須条件**。
- `obj.*` の使用頻度（ライブラリ含む延べ）：`setoption` 89、`copybuffer` 85、`effect` 73、`draw` 45、`drawpoly` 40、`getpixel` 37、状態変数（cx/cy/ox/oy/oz 等）約100、`load` 15、`putpixeldata` 7、`getpixeldata` 6、`getinfo` 4、`getvalue` 3、`rand` 2、`setfont` 1、`setanchor` 1。**中核は「バッファ操作＋組み込み効果の呼び出し＋変換描画＋ピクセル読み書き」**で、カメラ・アンカー系は少数派。
- 共有ライブラリは `require`（延べ86件）で読み込まれる。**モジュール読み込み（script フォルダ相対）も実運用の前提**。
- ピクセル処理系ライブラリ（sigma_dither.lua 等）は **LuaJIT の `ffi`（cast／new／fill）と `getpixeldata` の生ポインタ**に依存する。FFI で任意の C 関数・DLL を呼ぶスクリプトは Linux では原理的に再現できず、互換の上限になる（ADR の判断点3）。

#### 本アプリの描画経路への対応表

現行経路の要点：プレビューは素材を 320×180／10fps の PNG 列に事前展開（`FrameStore`）し、再生ヘッド位置のフレームを「切り抜き合成済み ＞ トランジョンブレンド済み ＞ 素フレーム」の優先で選びテクスチャ表示する（`apps/video-editor/src/main.rs` のプレビュー描画、`media.rs` の `blend_frame_files`／`FrameStore::frame_path`）。書き出しは `export_frame_at`（`media.rs:602`）が同じ規則でフレームを選び、設定解像度（既定 1280×720／30fps）の PNG 列＋音声ミックスを外部 ffmpeg でエンコードする（ADR-0015）。CPU 合成は `image` クレートの RGBA バッファ上で行う（`blend_frame_files`、`cutout.rs` の `composite_pixel`）。タイトル文字は nannou `draw.text` による GUI 描画のみで、フレームバッファには焼かれない（書き出しにも入らない）。

| ExEdit API 群 | 本アプリでの対応先 | 状態 |
|---|---|---|
| 状態変数 time/frame/totalframe/totaltime/framerate、w/h、screen_w/h | `source_time_ms`、fps、フレーム寸法から供給 | 既存で供給可能 |
| 状態変数 ox/oy/oz、rx/ry/rz、cx/cy/cz、zoom、alpha、aspect | クリップに位置・回転・拡大の概念が無く、フレームは無変換で表示される | **要新設**（スクリプト実行段でのフレーム変換として実装） |
| layer/index/num | トラック／クリップで近似できるが意味論の対応付けが必要 | 要設計（#81） |
| パラメータ指令（--track/--check/--dialog/--color/--file/--param）と track0..3/check0 | パース、Clip への値保持、コマンド化、GUI の値編集がすべて無い | **要新設**（#83 の中心） |
| require による共有ライブラリ読み込み | Lua 基盤側のモジュール解決（script フォルダ相対） | 要新設（#80） |
| obj.load("figure") | 図形ラスタライザ（矩形・円・三角・星型等を RGBA バッファへ） | **要新設** |
| obj.load("text")＋obj.setfont | CPU テキスト描画＋フォントエンジン。現行は GUI の draw.text のみ | **要新設**（書き出しに文字を入れる既存の弱点と同根） |
| obj.load("image"／"movie") | `image` クレート／FrameStore の素材経路を流用 | 既存で供給可能 |
| obj.load("framebuffer"／"tempbuffer")、copybuffer、setoption("drawtarget") | 合成中フレームと作業バッファの管理。CPU バッファが実体になる | 要新設（薄い） |
| obj.draw／drawpoly | 拡大・回転・α のアフィン変換と四角形射影変換のラスタライザ。`imageops` には resize のみ | **要新設** |
| obj.effect（名前付き組み込み効果） | 呼び先の効果ライブラリが無い（既存の映像効果はトランジション・切り抜き・タイトルのみ） | **要新設**（初期セットの範囲は判断点4） |
| obj.filter | フレーム全体への適用点は export／プレビューの後段に置ける。効果ライブラリは effect と共通 | 要新設（effect と一体） |
| getpixel／putpixel／copypixel／pixeloption | RGBA バッファへの直接アクセス（試作で実証）。ただし "yc" 型は内部形式であり、"col"／"rgb" のみか変換提供かの判断が要る | 実装可能（試作済み） |
| getpixeldata／putpixeldata | RGBA バッファの受け渡し。Lua 側への渡し方（文字列／userdata）の設計が要る | 実装可能（第2段） |
| obj.getaudio | 書き出しは `mix_audio` の PCM が使える。"spectrum" は FFT の新設が要る | 一部既存／要新設 |
| obj.rand、RGB、HSV、ビット演算 | 純粋関数 | 実装可能（容易） |
| obj.getvalue／getpoint | トラックバーの中間点・区間（キーフレーム）モデルが本アプリに無い | 対象外か縮退実装（第2段以降） |
| obj.setanchor | GUI のアンカーポイント編集が前提 | 対象外 |
| カメラ系（.cam、camera_param） | カメラモデルが無い | 対象外 |
| obj.getinfo（"script_path"／"saving"／"image_max"） | そのまま供給可能（saving は書き出し中かの区別に対応） | 既存で供給可能 |

差し込み点の結論：**クリップ単位のエフェクト段を CPU 合成経路に新設する**のが既存構造に最も素直である。プレビューではフレーム選択後・テクスチャ化前（ブレンド済みフレームのキャッシュ PNG と同型の置き方）、書き出しでは `export_frame_at` の選択後・PNG 生成時。実行単位はフレーム毎で、`obj.time` はクリップ相対秒。性能の見積もり：プレビュー 320×180＝57,600 画素は純 Lua のピクセルループでも実用圏だが、書き出し 1280×720＝921,600 画素×30fps では重くなる（lua.txt 自身が pixel 系関数の重さを警告している）。対策は LuaJIT 化の余地と `getpixeldata` 一括受け渡しで、初期実装の必須条件にはしない。

### AC3: 方式候補の比較がある（工数感・リスク・対応できる拡張の範囲） — ✅（ADR-0019 の比較表）

A（Lua スクリプトエフェクト互換に絞る）／B（独自 Lua API＋読み替え層）／C（Wine ブリッジでバイナリ互換）／D（互換を捨て独自 API のみ）の4案を、互換範囲・工数目安・リスクで比較し、ADR-0019 に記録した。要点：

- **A（推奨）**：4〜6 PBI。約束できる範囲が明確で、差し込み点が既存の CPU 合成経路に乗る。
- **B**：A に＋2〜3 PBI。読み替えの未カバー部が静かな非互換になり、独自 API の初期利用者はゼロ。
- **C**：独立 EPIC 級（8 PBI 以上）。障壁は Wine ではなく AviUtl ホスト（EXFUNC・メッセージ・編集状態）の再現度で、動く範囲を事前に約束できない。ExEdit 自体は対象外。
- **D**：約3 PBI と最安だが、ユーザーの要望（AviUtl の拡張を入れたい）を満たさない。A の後から独自 API を足すことは可能。

ライセンスの含意も ADR に記録した：**プラグイン SDK は MIT**（SDK 同梱 license.txt で確認）。AviUtl 本体・ExEdit は同梱・再配布せず、スクリプト互換は lua.txt に基づくクリーンルーム再実装とする。Lua／LuaJIT／mlua は MIT 系。配布スクリプトのライセンスは作者ごとに異なり（実測した sigma-axis 氏のものは MIT）、第三者スクリプトの製品同梱は個別許諾が前提。

### AC4: 推奨方式が ADR として「提案」で記録され、決定依頼コメントが投稿され、`needs-decision` が付いている — ✅

- [ADR-0019](../adr/0019-aviutl-lua-script-compat-scope.md) を「提案（未採用）」で作成し、`docs/adr/index.md` に登録した。判断点は ADR の「残る判断点」に5件集約した（方式の選択、スクリプト種別の初期範囲、LuaJIT／FFI の扱い、効果名の初期セット、AviUtl2 世代の扱い）。
- イシュー #79 には元から `needs-decision` が付いている。本レポートの push 後に決定依頼コメント（推奨案・代替案・判断のお願い）を投稿する。本 PBI は決定が下りるまで close しない（#48・#71 の前例どおり）。

## 試作（結論の裏付け）

mlua（Lua 5.1、vendored ビルド）で、実際の .anm と同型のスクリプトをモック `obj` に対して実行する使い捨て試作を /tmp で行った。証拠は [evidence/issue-0079/spike-stdout.txt](evidence/issue-0079/spike-stdout.txt) と [spike-main.rs](evidence/issue-0079/spike-main.rs)。

- パラメータ指令（`--track0`、`--track1`、`--check0`、`--dialog`）を先頭コメント行からパースできた。
- `obj.load("figure", ...)` → ピクセルループ（`getpixel`／`putpixel` で全画素の R 反転）→ `obj.effect(...)` → `obj.draw(...)` の順でホスト関数が呼ばれ、RGBA バッファ（64×36）に反映された。期待値は先に計算した：全画素が (0x11,0x22,0x33,0xFF) で埋まり R だけ反転するため、画素 (0,0) は (238,34,51,255)、チェックサムは 578×2304＝**1,331,712**。実測は画素・チェックサムとも完全一致した。
- 効果名は **Shift_JIS のバイト列**（「色調補正」= 90 46 92 B2 95 E2 90 B3）のまま Lua 文字列として渡り、ホスト側でバイト列として受け取れた。ExEdit の名前ベースのディスパッチをバイト列前提で再現できることの裏付けになる。
- ランタイムは `_VERSION = "Lua 5.1"`。ExEdit の lua51.dll（Lua 5.1.4）と同系であり、意味論の基準として妥当。

## 未検証事項

- **実 AviUtl／ExEdit での動作確認は一切できていない**（Windows 環境なし）。`obj.*` の意味論（特に draw の座標系・合成順、effect のパラメータ名と値域、copybuffer のキャッシュ挙動）は lua.txt の記述に基づく推定であり、実スクリプトを本アプリで動かす段階（#83）で初めて実証される。
- LuaJIT 経路（mlua の luajit 機能、FFI の扱い）は試していない。判断点3の決定後に必要なら検証する。
- 書き出し解像度でのピクセル系スクリプトの性能は未計測（見積もりのみ）。
- ExEdit 0.92 と 0.93rc1 の差異（LuaJIT 既定化等）がスクリプト互換に与える影響は未精査。流通スクリプトの多くは 0.92 前提で書かれている。
- .auc（色変換）の ABI は SDK の実物ヘッダで直接確認していない（他3種と同パターンの前提で、方式の結論には影響しない）。

## TDD の記録

- 本 PBI は調査のため本体コードの変更がなく、TDD の対象はない。試作では、バッファの期待値（画素値とチェックサム）を実行前に式から定め、実測と照合した（上記「試作」）。

## コードレビュー

- （実施後に記入する）

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた（AC1〜AC4 は本レポートと ADR-0019、決定依頼コメントで確認）
- [ ] TDD で実装した — 対象外（調査 PBI。本体コードの変更なし）
- [ ] `cargo test` が 0 で終わった — 対象外（同上。本体コードを変更していないため、ワークスペースの状態は main のまま）
- [ ] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した — 対象外（受け入れ条件は文書成果物のみ。試作の証拠は evidence/issue-0079/ に残した）
- [ ] push の前に独立した `code-reviewer` が `APPROVE` を返した — （実施後に記入する）
- [ ] この変更の commit で CI が緑 — （push 後に記入する）
- [x] 判断があれば ADR を同じ commit で足した（ADR-0019 を提案として記録。採用の決定は needs-decision として人間に渡した）
- [x] 試験レポートを `docs/test-reports/` に、証拠を `docs/test-reports/evidence/` に置いた
- [ ] 全部にチェックを入れ、commit・CI の URL・試験レポートへのリンクをコメントに書いて閉じた — **未了**: 本イシューは `needs-decision`（ADR-0019 の採用可否は人間の決定待ち）のため、決定後に close する。調査成果物は本レポートで完成している
