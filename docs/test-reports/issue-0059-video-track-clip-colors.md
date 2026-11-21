# 試験レポート: Issue #59 動画編集 GUI トラック／クリップの色付け（固定パレット）

- 対象: `apps/video-editor`（timeline の表示色解決、palette パネル、
  main の配線）
- 前提: ADR-0013（固定パレット方式）。コアの保持・コマンド・MCP は
  #58 で整備済みで、本 PBI は動画編集 GUI の操作と描画のみ。
- 結果: ワークスペース全体 149 件パス（144 → 149、本 PBI で +5）

## 実施内容

- `timeline.rs`:
  - `PALETTE`: コアの `ColorTag` 8種に対応する表示色（動画編集の
    暗い画面に合わせた RGB 0.0–1.0）。保持は識別子のみで、RGB は
    アプリ側の表現として置く（ADR-0013）。
  - `palette_color(tag)`、`clip_display_color(color, kind)`
    （ユーザー色優先、未設定は従来の種別色）、
    `track_display_color(color, kind)`（ヘッダーの色帯用、同規則）。
- `palette.rs`（新規、GUI 非依存）: トラック追加エリアの下に置く
  パネルのレイアウトと `hit_palette`。DAW #58 と同じ操作系
  （TRACK 行 / CLIP 行 × 「なし」+ 8色）。
- `main.rs`:
  - `apply_palette_click`: TRACK 行は選択中トラック、CLIP 行は
    選択中トラックの再生位置にあるクリップへ `SetTrackColor` /
    `SetClipColor` を `apply` 経由で発行（DAW #58 と同じ対象規則）。
    対象が無い場合は理由を標準出力に残す。
  - 描画: クリップ面は `clip_display_color`、トラックはヘッダー
    左端に 6px の色帯（`track_display_color`。未設定時は種別色が
    出るため、設定すると帯の色が変わる形で反映が分かる）。
    パネルは現在の色のスウォッチに明るい枠を付ける。

## テスト（TDD の記録）

先に書いたテスト（video-editor バイナリ 29 → 34 件）:

- `timeline` の `clip_display_color_prefers_user_color_over_kind_color` /
  `track_display_color_falls_back_when_unset`（イシュー指定のテスト名）、
  `palette_covers_all_tags`
- `palette` の `hit_palette_maps_swatches` /
  `panel_sits_below_track_add_area`
  - red: 新規関数を除いた状態で
    `error[E0425]: cannot find value PALETTE in module timeline` /
    `cannot find function track_display_color` を確認。
  - palette.rs の2件はテストと実装を同一ファイルで作成したため
    red の記録はない（初回実行で green）。timeline の3件は
    上記のとおり red → green を確認済み。

green 後の結果: ワークスペース全体 149 件パス。
`./scripts/verify.sh` は終了コード 0。CI（run 44947215965）も緑。

## GUI 証拠（Xvfb + lavapipe、実ディスプレイ未検証）

`docs/test-reports/evidence/issue-0059/drive.sh` で実アプリを操作した
（デモ構成: V1 / A1 / V2、パネルは3トラック時に上端 y=228）。

- 初期: V1 ヘッダー帯 (3, 48) = (64, 115, 217) = 映像の種別色
  （未設定フォールバック）。
- CLIP 行 Lavender → ダンプに `"color": "lavender"`、対象クリップ
  （opening）の面ピクセルが (166, 140, 242) = Lavender 表示色。
  対象クリップはダンプから特定して矩形を計算し直して確認した。
- TRACK 行 Coral → ダンプに `"color": "coral"`、V1 ヘッダー帯が
  (242, 127, 89) = Coral 表示色。
- TRACK 行「なし」→ 最終ダンプで V1 の `"color": null`、帯も
  種別色 (64, 115, 217) に復帰。
- 撮影上の注意: 再生位置が V1 のクリップ外（隙間 2000〜2500ms や
  末尾）だと CLIP 行の対象が無いため、ドライブはダンプに色が
  現れるまでクリックを繰り返す。Coral の緑成分は 0.50×255 の
  丸めで 127 になる（期待値は実測に合わせた）。

## コードレビュー

round 1: **APPROVE**（Major / Minor の是正指摘なし）。受け入れ条件
4件すべてに証跡が対応すること、状態変更がコマンド経由のみである
こと、再生中 ACCENT 上書きは動画編集側に該当する状態色がなく
イシューの描画順序（ユーザー色 → 種別色）どおりであることを
レビュアーが確認した。非ブロッキングの観察として、触るファイル
一覧外の新規モジュール `palette.rs` は #58 と同じ分離でスコープ内
とされた。

## 実装レベルの判断（ADR-0006 に基づき記録）

- トラック色の反映先はヘッダー左端の色帯（DAW と同じ表現）。
  未設定でも種別色の帯を出すことで、帯が「そのトラックの表示色」
  そのものになり、設定の反映が帯の色の変化として一目で分かる。
- パレットの RGB は動画編集の暗い画面に合わせて DAW とは別の
  値にした（識別子は共通。ADR-0013 は RGB をモデルに持たない
  ため、アプリごとの表現は自由）。
