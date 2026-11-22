# 試験レポート： Issue #39 DAW でトラックをミュート／ソロにできる

- 日付: 2026-11-22
- 実行者: エージェント（デリバリーループ、並行レーンA＝DAW 担当）
- 対象: Track に muted/solo を追加し、SetTrackMute/SetTrackSolo
  コマンド、再生ミックスの可聴判定、ミキサーの M/S ボタン、
  MCP ツールを実装する。ミュート優先、複数ソロ可。
- 実装コミット: `614c3bc`
- CI: `614c3bc` 緑 https://github.com/Uncheese1Unko/rust-studio/actions/runs/45006622001
- 環境: Yoko の VM（Linux、ヘッドレス）。表示確認は Xvfb（:91）＋Mesa
  lavapipe（ソフトウェアレンダリング）。**実ディスプレイでの確認は未検証。**
- テスト件数: ベースライン 228 件 → 234 件（新規6件: コアのコマンド
  1・永続化1、audio 2、mixer 1、MCP 1）。削除・弱体化なし。

## 受け入れ条件ごとの結果

### AC1: SetTrackMute/SetTrackSolo ができ、ダンプとプロジェクト JSON で観測・永続化できる — ✅

- Track に `muted`/`solo`（serde default false）を追加。
  コマンドテスト `set_track_mute_and_solo_apply`（適用と、
  存在しないトラックでの TrackNotFound＋状態不変）と永続化
  テスト `mute_and_solo_survive_save_load_and_legacy_json_defaults`
  （保存・復元と、フィールドなし旧 JSON が false で読めること）。

### AC2: ミュートはミックスに出ず、ソロがあればソロ以外は出ない（ミュート優先） — ✅

- 可聴判定は純粋関数 `audio::is_track_audible(track, any_solo)`
  （ミュート→不可聴を先に判定）。render_mix が全体の any_solo を
  1回計算して skip する。メトロノーム付きレンダリングも
  render_mix に委譲するため同じ判定が効く。
- テスト（イシュー指定名）`muted_track_is_silent_and_solo_isolates`:
  440Hz/880Hz の2トラックを Goertzel で分離し、ミュート消音・
  ソロ分離・ミュート>ソロを確認。真理値表テスト
  `track_audibility_rules` も追加。

### AC3: ミキサーの各トラック帯に M/S ボタンがあり、状態が色で分かる — ✅

- `mixer::mute_rect`/`solo_rect`/`hit_mixer_button`/
  `toggle_command`（現在状態を反転するコマンドを生成）。
  クリックは History 経由で適用し、プロジェクトファイルへ保存。
- 配置の判断: 当初はパンスライダーの右（x=804〜）を予定したが、
  選択中トラックの大きな現在値表示（#25、x≈872 中心）と重なる
  ため、ヘッダー右の空き帯（x=126〜174）に置いた。
- 実アプリ証拠: M ボタンのピクセルが PANEL_SUNK→ACCENT、
  S ボタンが PANEL_SUNK→TEAL に変わり、保存 JSON にも
  muted/solo が残る（`evidence/issue-0039/`）。

### AC4: MCP から切り替えられ、ダンプで観測できる — ✅

- `set_track_mute`/`set_track_solo` を tools/list に追加
  （コマンド19種）。実行は既存の汎用ディスパッチ（tool名＝
  コマンドの type タグ）。テスト
  `set_mute_solo_tools_apply_via_tools_call` が tools/call →
  get_project ダンプでの観測まで検証。

## TDD の記録

- 上記のテスト6件を先に書き、`SetTrackMute` バリアントなし
  （E0599）・`muted`/`solo` フィールドなし（E0609）・
  `is_track_audible` 未定義（E0425）で落ちることを確認してから
  実装した。

## 実装中の記録（正直な記録）

- Track リテラルへの muted/solo 追加を機械置換した際、Clip
  リテラルにも誤って混入し、コンパイルエラーで検出して除去した。
- ボタンのクリック処理で当初プロジェクトファイルへの保存を
  呼び忘れ、ドライブの「保存 JSON」確認で検出して
  `save_project_file` を追加した（他の編集経路と同じ扱い）。
- ドライブのマーカーは整形 JSON の空白（`"muted": true`）に
  合わせた。S ボタンのピクセル確認点は角丸のアンチエイリアスを
  避けて内側に置いた。

## コードレビュー

- 独立 code-reviewer（新規サブエージェント、イシュー番号と diff
  範囲 `14abdf0..614c3bc` のみを提示）に依頼し、**ラウンド1で
  APPROVE**。ゲートもレビュアーが自分で実行して全緑（234 件）。
- レビュアーの注記: diff 範囲に他レーンの ADR-0014 コミット
  （94299ae、ユーザー決定の文書化）が含まれるのは、レビュー
  基準点がそのコミットより前だったためで、本実装の変更ではない。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red の記録は上記「TDD の記録」）
- [x] `cargo test` が 0 で終わった（`./scripts/verify.sh` 緑、234 件）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断があれば ADR を同じ commit で足し、関係する文書とルールを
      直した（ボタンの配置判断は本レポートに記録。新規 ADR なし）
- [x] 試験レポートと証拠を置いた（本ファイルと `evidence/issue-0039/`）
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
      （close コメントで実施）
