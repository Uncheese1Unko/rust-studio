# 試験レポート： Issue #32 DAW でクリップを分割できる（SplitClip）

- 日付: 2026-11-22
- 実行者: エージェント（デリバリーループ、並行レーンA＝DAW 担当）
- 対象: コアに SplitClip コマンドと Clip の `offset_ms` を追加し、DAW の
  レンダリング・波形表示・GUI（S キー）・MCP から分割できるようにする。
- 実装コミット: `969c70f`（本体）、`e1382fb`（レビューの非ブロッキング
  観察への対応：tools/list 期待リストに split_clip を追加）
- CI: `e1382fb` 緑 https://github.com/Uncheese1Unko/rust-studio/actions/runs/45000082035
- 環境: Yoko の VM（Linux、ヘッドレス）。表示確認は Xvfb（:91）＋Mesa
  lavapipe（ソフトウェアレンダリング）。**実ディスプレイでの確認は未検証。**
- テスト件数: ベースライン 180 件 → 188 件（新規8件：core 5、MCP 1、
  DAW 2）。削除・弱体化なし。

## 受け入れ条件ごとの結果

### AC1: SplitClip で左（元の id）と右（新規 id）に分かれ、右の素材内開始位置が進む — ✅

- `crates/studio-core/src/command.rs` に `Command::SplitClip { clip_id, at_ms }`
  を追加。左は元の id のまま `duration = at_ms - start_ms`、右は新規 id で
  `offset_ms = 元の offset_ms + (at_ms - start_ms)`。name / source / gain /
  cutout / pitch_semitones / color は右へ引き継ぐ。
- `crates/studio-core/src/model.rs` の `Clip` に `offset_ms`（`#[serde(default)]`、
  旧 JSON では 0）を追加。
- テスト: `split_clip_creates_right_clip_with_advanced_offset`、
  `split_clip_twice_accumulates_offset`（2回分割で offset が 1000→3000 に累積）、
  `split_clip_command_round_trips_through_json`、
  `legacy_json_without_offset_loads_as_zero` が通過。

### AC2: 分割位置がクリップ範囲外なら失敗し、状態が変わらない — ✅

- 検証は id 採番より前に行い、`at_ms <= start || at_ms >= end` は
  `InvalidRange`、未知のクリップは `ClipNotFound` を返す。
- テスト `split_clip_outside_range_fails_and_keeps_state`: at_ms ∈
  {0（=start）, 5000（=end）, -1, 6000} と未知 id で `dump_json()`
  （next_id を含む）が変化しないことを確認。

### AC3: GUI で再生ヘッド位置で分割でき、分割前と同じ音が続けて鳴る — ✅

- GUI: `apps/daw/src/main.rs` の `handle_split_key`。S キーの押下エッジで、
  選択中トラックの再生ヘッド位置にあるクリップを再生ヘッド位置で分割する。
  S はサンプラーのノートキーと重なるため、再生ヘッドがクリップ内部に
  ある場合だけ分割側がキーを消費し、それ以外は従来どおりサンプラー
  演奏に渡る（イシューの実装方針どおり）。
- 音の継続性: `apps/daw/src/audio.rs` の `render_mix` が素材位置を
  `t - start_ms + offset_ms` で読むよう変更。テスト
  `render_mix_matches_before_and_after_split`（分割前後のミックスの
  最大差 < 1e-6）で担保。波形表示も `timeline::waveform_source_frac`
  で offset を反映（単体テスト `source_frac_starts_at_clip_offset`）。
  波形が offset を見ていないと右クリップの表示だけ素材先頭に戻るため、
  本 PBI の範囲内で合わせて直した。
- 実アプリ証拠（`evidence/issue-0032/drive.sh`、Xvfb :91）:
  プロジェクトファイル共有モードで 9000ms の単一クリップを開き、S キーで
  1754ms 地点を分割。stdout の「after split」ダンプで2クリップ化と
  右クリップの `offset_ms = 1754` を確認し、スクリーンショット
  （`shot1-before.png` / `shot2-after.png`）でも境界が目視できる。
  ピクセル検証: 分割点左側の面が再生中色 ACCENT (255,46,136) →
  通常面 ACCENT_SOFT (255,199,222) に変わり、右側は ACCENT のまま
  であることを確認（`drive.sh` 内の python チェックが OK）。

### AC4: MCP からも分割でき、ダンプで観測できる — ✅

- `crates/studio-mcp` に `split_clip` ツール定義を追加（実行経路は
  既存の tool 名→Command 汎用変換＋History）。
- テスト `split_clip_tool_applies_via_tools_call`: tools/call で分割 →
  ダンプで右クリップの offset 1500 / duration 2500 を確認。端位置の
  再分割はエラーになり、ダンプが変わらないことも確認。

## TDD の記録

- `split_clip_creates_right_clip_with_advanced_offset` 他 core テスト5件と
  MCP テストを先に書き、`error[E0599]: no variant named SplitClip` /
  `error[E0609]: no field offset_ms` で落ちることを確認してから実装した。
- 波形ヘルパー `waveform_source_frac` のテスト
  `source_frac_starts_at_clip_offset` は
  `error[E0432]: unresolved import super::waveform_source_frac` で
  落ちることを確認してから実装した。
- `render_mix_matches_before_and_after_split` は render_mix の offset
  反映を入れる前に書いた。その感度（offset を無視する実装では落ちる
  こと）は、独立レビュアーが変異版で実験して確認した
  （max_diff = 2.06e-4 > 許容 1e-6）。

## コードレビュー

- 独立 code-reviewer（新規サブエージェント、イシュー番号と diff 範囲
  `24e9ec2..969c70f` のみを提示）に依頼し、**ラウンド1で APPROVE**。
- レビュアーは等価性テストの感度も独自に検証した（offset を無視する
  変異版では max_diff = 2.06e-4 > 許容 1e-6 となり、テストが実際に
  落ちることを確認）。
- 非ブロッキング観察1件：`tools_list_exposes_all_commands` の期待
  リストに `split_clip` が含まれていない（contains チェックのため緑）。
  その場で期待リストへ追加した（コミット `e1382fb`）。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red の記録は上記「TDD の記録」）
- [x] `cargo test` が 0 で終わった（`./scripts/verify.sh` 緑、188 件）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断があれば ADR を同じ commit で足し、関係する文書とルールを
      直した（新規の設計判断なし。S キーの競合回避はイシューの
      実装方針どおり）
- [x] 試験レポートと証拠を置いた（本ファイルと `evidence/issue-0032/`）
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
      （close コメントで実施）
