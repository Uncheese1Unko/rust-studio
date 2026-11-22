# 試験レポート： Issue #34 DAW でクリップにフェードイン／フェードアウトを付けられる

- 日付: 2026-11-22
- 実行者: エージェント（デリバリーループ、並行レーンA＝DAW 担当）
- 対象: コアにフェード（fade_in_ms / fade_out_ms）と SetClipFade
  コマンドを追加し、render_mix の直線包絡、GUI のコーナードラッグと
  傾斜描画、MCP 公開を行う。
- 実装コミット: `a349e9d`
- CI: `a349e9d` 緑 https://github.com/Uncheese1Unko/rust-studio/actions/runs/45001850942
- 環境: Yoko の VM（Linux、ヘッドレス）。表示確認は Xvfb（:91）＋Mesa
  lavapipe（ソフトウェアレンダリング）。**実ディスプレイでの確認は未検証。**
- テスト件数: ベースライン 192 件 → 200 件（新規8件：core 3、
  render_mix 2、edit 2、MCP 1）。削除・弱体化なし。

## 受け入れ条件ごとの結果

### AC1: コアにフェードと SetClipFade ができ、不正な値では状態が変わらない — ✅

- `Clip` に `fade_in_ms` / `fade_out_ms`（`#[serde(default)]`、旧 JSON
  は 0）を追加。`SetClipFade` は負値とクリップ長超過を `InvalidRange`
  で弾き、全検証が代入より前にあるため失敗時に状態は変わらない。
- テスト: `set_clip_fade_applies_and_validates`（正常設定＋不正4ケースで
  `dump_json()` 不変）、`legacy_json_without_fades_loads_as_zero`。

### AC2: 再生ミックスで境界付近の振幅が直線的に増減する — ✅

- `audio::fade_envelope`（クリップ内位置に対する直線包絡、重なり区間は
  小さい方）を render_mix のサンプル加算に乗算。
- テスト（イシュー指定名）: `render_mix_applies_linear_fade_in` —
  定数 0.8 の素材＋フェードイン 1000ms で 0ms→0、250ms→0.2、
  500ms→0.4、1000ms→0.8 を検証。`render_mix_applies_linear_fade_out`
  も対称に検証。

### AC3: GUI でクリップの左右上端をドラッグしてフェード長を変えられ、波形に傾斜が描かれる — ✅

- ヒットテスト: 上端帯（12px）かつ左右端のときだけ
  `ClipFadeInCorner` / `ClipFadeOutCorner` を返し、それ以外は従来の
  トリム端にフォールスルー（テスト
  `hit_test_top_corners_are_fade_handles` が同じ x で y=28→フェード、
  y=48→トリムを検証）。ドラッグは `DragKind::FadeIn/FadeOut` が
  0..=duration にクランプした SetClipFade を生成する。
- 傾斜描画は `apps/daw/src/main.rs` の描画部に実装した（イシューの
  「触るファイル」は timeline.rs としていたが、クリップ描画は main.rs
  に集約されているため。レビューで非ブロッキング所見として記録）。
- 実アプリ証拠（`evidence/issue-0034/drive.sh`、Xvfb :91）:
  左上コーナーを約1500ms 分ドラッグ → 保存されたプロジェクトの
  `fade_in_ms = 1476`（fade_out は 0 のまま）。ピクセル検証は実測の
  フェード長から対角線の 1/4 点・3/4 点を計算し、両点の近傍に
  傾斜線の暗色ピクセルがあること（ドラッグ前は無し）を確認した。
  ※検証中、再生ヘッドの縦線が「暗ピクセル」として混入したため、
  2点一致方式に変えた経緯がある（drive.sh 内コメント参照）。

### AC4: MCP からフェードを設定でき、ダンプで観測できる — ✅

- `set_clip_fade` ツールを公開。テスト
  `set_clip_fade_tool_applies_via_tools_call`: 設定→ダンプで観測、
  長さ超過はエラーでダンプ不変。tools/list の期待リストにも追加。

## 実装上の判断（記録）

- **分割時のフェード**: 分割点は切り口として扱い、左クリップは
  fade_in のみ（左の長さでクランプ）、右クリップは fade_out のみ
  （右の長さでクランプ）を引き継ぐ。テスト
  `split_clip_keeps_fades_on_the_outer_edges_only` で担保。
- **トリム時のフェード**: TrimClip で両フェードを新しい長さに
  クランプし、不変条件（0 <= fade <= duration）を保つ。
- **複製（#33）への波及**: ClipPlacement の追随コマンドに
  SetClipFade を追加し、複製でもフェードが引き継がれるようにした。

## TDD の記録

- core テスト3件を先に書き、`no variant named SetClipFade` /
  `no field fade_in_ms` で落ちることを確認。音声・edit・MCP の
  テストも同様に `SetClipFade` / `ClipFadeInCorner` 不在のコンパイル
  エラーで落ちることを確認してから実装した。

## コードレビュー

- 独立 code-reviewer（新規サブエージェント、イシュー番号と diff
  範囲 `bbc05ee..a349e9d` のみを提示）に依頼し、**ラウンド1で
  APPROVE**。レビュアーは分割・トリムで不変条件が壊れないことも
  grep で確認し、ゲートも自分で実行して全緑（200 件）を確認。
- 非ブロッキング所見: 傾斜描画の置き場所がイシューの「触るファイル」
  （timeline.rs）と異なる点（上記 AC3 に記録）。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red の記録は上記「TDD の記録」）
- [x] `cargo test` が 0 で終わった（`./scripts/verify.sh` 緑、200 件）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断があれば ADR を同じ commit で足し、関係する文書とルールを
      直した（判断は上記「実装上の判断」に記録。ADR が要る規模の
      設計判断はなし）
- [x] 試験レポートと証拠を置いた（本ファイルと `evidence/issue-0034/`）
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
      （close コメントで実施）
