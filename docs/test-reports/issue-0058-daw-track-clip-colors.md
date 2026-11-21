# 試験レポート: Issue #58 DAW トラック／クリップの色付け（固定パレット）

- 対象: `crates/studio-core`（ColorTag / SetTrackColor / SetClipColor）、
  `crates/studio-mcp`（ツール定義）、`crates/studio-api`（テストで確認）、
  `apps/daw`（theme の PALETTE、timeline の色解決、palette パネル）
- 前提: ADR-0013「トラック／クリップの色付けはテーマ固定パレット
  方式とする」（decider: user、2026-11-22）。自由色は採らず、パレット
  識別子で保持し、再生中は ACCENT で上書きする。
- 結果: ワークスペース全体 142 件パス（132 → 142、本 PBI で +10）

## 実施内容

- コア（`crates/studio-core`）:
  - `ColorTag` 列挙（Pink / Coral / Peach / Lemon / Mint / Teal /
    Sky / Lavender、`snake_case` で直列化）を `model.rs` に追加し、
    `Track.color` / `Clip.color: Option<ColorTag>` を
    `#[serde(default)]` で追加（旧形式 JSON は None で読める）。
  - `Command::SetTrackColor { track_id, color }` /
    `SetClipColor { clip_id, color }`（None = 未設定に戻す）を追加。
    未知の id は `TrackNotFound` / `ClipNotFound` で状態不変。
    AddTrack / AddClip の生成時は `color: None`。
- MCP: `set_track_color` / `set_clip_color` のツール定義を追加
  （color は8タグの列挙 + null、必須は id のみ）。実行は既存の汎用
  経路（ツール名 → Command デシリアライズ → apply）のまま。
- HTTP API: 汎用 Command 経路でそのまま動くことをテストで確認
  （API 側の変更なし）。
- DAW（`apps/daw`）:
  - `theme.rs`: `PALETTE`（タグ → 面色・縁色の組。ADR-0012 の色相に
    合わせた淡色で、面・縁とも ACCENT そのものは含めない。Pink 面 =
    ACCENT_SOFT、Mint = (MINT, TEAL)）と `palette_colors(tag)`、
    パネルラベル `05 COLOR`。
  - `timeline.rs`: `clip_colors_for(color, kind, playing)`。
    再生中は色の有無によらず (ACCENT, ACCENT)、色ありはパレット色、
    未設定は従来の種別色にフォールバック。
  - `palette.rs`（新規、GUI 非依存）: トランスポート下のパネルの
    レイアウトと `hit_palette`（TRACK 行 / CLIP 行 ×
    「なし」+ 8色のスウォッチ）。
  - `main.rs`: パネル描画（現在の色のスウォッチにウォームブラックの
    枠）、クリックで `apply_palette_click` がコマンドを発行
    （TRACK 行 = 選択中トラック、CLIP 行 = 選択中トラックの再生位置
    にあるクリップ。対象が無い場合は理由を標準出力に残す）。
    トラック色はタイムラインのヘッダー左端とミキサーストリップ
    左端に 6px の帯でも表示する。

## テスト（TDD の記録）

先に書いて失敗を確認したテスト:

- コア（command_tests 15 → 18 件）:
  `set_track_color_round_trips_through_json` /
  `set_clip_color_applies_and_validates` /
  `legacy_json_without_color_loads_as_unset`
  - red: `error[E0432]: unresolved import studio_core::ColorTag`、
    `no variant named SetTrackColor`。実装後は
    `set_track_color_round_trips_through_json` がダンプ整形
    （`"color": "mint"` の空白あり表記）の assert で1件落ち、
    テスト側の期待文字列を直して green。
- MCP（15 → 16 件）: `set_color_tools_apply_via_tools_call` と
  tools/list の期待値更新。
  - red: `tools_list_exposes_all_commands` のみ失敗（呼び出し自体は
    汎用経路で先に通ったため、定義追加が本丸だった）。
- API（9 → 10 件）: `set_track_color_command_applies_via_api`
  （API 無変更で通過）。
- DAW（60 → 65 件）: `theme::palette_tests::
  palette_has_all_tags_and_excludes_accent`、`timeline::color_tests::
  clip_color_falls_back_to_kind_color_when_unset` /
  `playing_clip_stays_accent_even_with_user_color`（イシュー指定の
  テスト名）、`palette::tests::hit_palette_maps_swatches` /
  `panel_sits_below_transport`
  - red: `error[E0425]: cannot find value PALETTE` /
    `cannot find function palette_colors`。
  - palette.rs の2件はテストと実装を同一ファイルで作成したため
    red の記録はない（初回実行で green）。コア・timeline・MCP は
    上記のとおり red → green を確認済み。

green 後の結果: ワークスペース全体 142 件パス。
`./scripts/verify.sh` は終了コード 0。CI（run 44934447667）も緑。

## GUI 証拠（Xvfb + lavapipe、実ディスプレイ未検証）

`docs/test-reports/evidence/issue-0058/drive.sh` で実アプリを操作した。
再生位置を決定的にするため、プロジェクトファイルモード
（`STUDIO_PROJECT_FILE`、main 0〜15000ms / sub 20000〜50000ms）で
起動している（`make_project.py` が生成）。

- CLIP 行 Sky クリック → ダンプに `"color": "sky"`（対象は再生位置の
  main クリップ）。
- **再生中は ACCENT で上書き**（受け入れ条件）: Sky を付けた main が
  再生中のスクリーンショット（02）で、面のピクセル (300, 32) =
  (255, 46, 136) = ACCENT。再生位置が main を抜けた後（04）は
  同じ点が (191, 220, 232) = Sky 面色になる。
- TRACK 行 Mint クリック → ダンプに `"color": "mint"`。ヘッダー帯
  (3, 48) とミキサー帯 (3, 138) のピクセルが (196, 237, 230) =
  MINT 面色。
- TRACK 行「なし」クリック → 最終ダンプでトラックの
  `"color": null`（クリア確認はダンプを JSON として解析して検証）。
- 撮影上の注意: ピクセル確認点は波形の描画帯（y 40〜55）を避けて
  y=32 に置いた。当初 y=48 で確認した際は波形色 (18, 181, 165) を
  拾って失敗扱いになったが、列スキャンで面色の帯を確認して
  修正した。また Vulkan ICD のファイル名は `lvp_icd.json`
  （`lvp_icd.x86_64.json` ではない。誤ると `NoAvailableAdapter` で
  起動に失敗する）。撮影は `-draw_mouse 0`。

## コードレビュー

round 1: **APPROVE**（Major / Minor の指摘なし）。受け入れ条件
6件すべてに証跡が対応すること、ADR-0013 準拠（固定パレット・
ACCENT を含まない・識別子保持・serde 既定）、状態変更がコマンド
経由のみであることをレビュアーが確認し、ワークスペース全体
142 件のテストを再実行して一致を確認した。

## 実装レベルの判断（ADR-0006 に基づき記録）

- パネルの配置はトランスポートパネルの下（サンプラー画面は画面下部
  固定のため衝突しない）。行の対象は TRACK = 選択中トラック、
  CLIP = 再生位置のクリップ（ピッチキーと同じ対象規則に揃えた）。
- クリップ色のクリアを GUI で行うには再生位置が対象クリップ内に
  ある必要があるため、証拠ではトラック色のクリアのみ GUI で確認し、
  クリップのクリアはコア／MCP のテストで担保した。
- トラック色の反映先はヘッダーとミキサーストリップの左端の帯。
  行全体の背景色は変えない（選択状態や交互色の判読性を保つため）。
