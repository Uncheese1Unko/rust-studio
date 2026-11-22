# Issue #68 試験レポート: メロディーをサンプラー楽器で再生できる

- 対象イシュー: #68（EPIC #66 [M3] の子）
- 前提: ADR-0016（ノートはクリップに統合、楽器はトラック単位で参照）
- 実装コミット: `97a4f81`（ノートと楽器のモデル、コマンド、MCP 公開とメロディー再生を実装する）、修正コミット `6b948aa`（レビュー指摘の修正）

## 実施内容

### モデルとコマンド（studio-core）

- `Note { pitch: u8, start_ms: i64, duration_ms: i64, velocity: u8 }` と
  `Instrument::Sampler { source: String, root_note: u8 }` を追加
  （`crates/studio-core/src/model.rs`）。`Clip.notes` と
  `Track.instrument` はどちらも `#[serde(default)]` で、旧プロジェ
  クト JSON を欠けたまま読める（後方互換テストで確認）。
- コマンド `SetTrackInstrument { track_id, instrument }` と
  `AddNote { clip_id, pitch, start_ms, duration_ms, velocity }` を追加。
  `AddNote` は ADR-0016 の不変条件（音高・ベロシティ 0–127、開始
  0 以上、長さ 1 以上、ノートがクリップ範囲内に収まる）を検証し、
  違反時は `InvalidRange` で状態不変。
- `SplitClip` は ADR-0016 の確定どおりノートを振り分け、境界を跨ぐ
  ノートは分割点で切断する（左は長さを切り詰め、右は残りを
  開始 0 のノートとして置く）。
- `TrimClip` は範囲外のノートを削除し、はみ出すノートの長さを
  新しい終端でクランプする（フェード／トランジションと同じ方針）。

### MCP 公開（studio-mcp）

- `set_track_instrument` と `add_note` のツール定義を追加し、
  tools/list の一覧テストと tools/call の適用テストで確認。
  HTTP API はコマンド JSON の汎用ラッパーのため追加作業なしで
  新コマンドを受け付ける。

### 再生（DAW `render_mix`）

- ノートのあるクリップは波形の代わりにノートで発音する
  （ADR-0016 のノートクリップ規則）。素材はノートの先頭から鳴らし、
  音高シフト量は `note.pitch - instrument.root_note`（半音）。
  ピッチ変更済み音声はクリップのピッチとキャッシュを共有する。
- ゲインは `clip.gain × velocity/127 × フェード包絡 × トラックの
  音量・パン係数`。ミュート／ソロは波形クリップと同じ規則で効く。
  楽器の無いトラックのノート、素材が読めないノートは無音に倒れる。
- `export_wav` は `render_mix` を使うため、書き出しにも同じ
  メロディーが載る。実アプリの音声マップ（`compute_audio_map`）に
  楽器の素材も含めたので、GUI 再生でもノートが鳴る。

## TDD の記録（red → green）

1. コア: 新テスト8件を先に書き、コンパイルエラーで red を確認
   （`unresolved imports studio_core::Instrument, studio_core::Note`、
   `no variant named AddNote / SetTrackInstrument`、
   `no field instrument on type &Track`）。実装後に green。
2. DAW 描画: ノート再生テスト6件を先に書き、5件失敗で red を確認
   （無音ガードの1件は現状でも通る回帰ガード）。実装後に green。
   なお +12 半音の測定は、既存のピッチテストと同じ理由（長い窓では
   周波数の微小な誤差で位相が崩れる）で `segmented_goertzel`
   （50ms 窓の平均）に切り替えて green にした。測定法の変更であり、
   実装の不具合ではない。
3. MCP: tools/list の一覧に2ツールを追加して red を確認
   （`tools_list_exposes_all_commands` 失敗）。ツール定義の追加で
   green。tools/call の適用は汎用デシリアライズのため定義前から
   通っており、適用テストは回帰ガードとして追加した。

## 検証結果

- `./scripts/verify.sh`: OK。ワークスペース全体のテストは
  280 件 → **298 件**（内訳: studio-core +10、DAW +7、studio-mcp +1。
  レビュー修正で追加した3件を含む）。
- 実バイナリ（`target/debug/daw --export-wav`）: MCP で生成した
  ノートプロジェクト（220Hz サイン素材、ルート 60、ノート
  60/64/72）を書き出し、窓ごとにピーク周波数を測定した。
  220Hz（シフト 0）→ 277Hz（+4 半音）→ 442Hz（+12 半音、440 近傍）、
  ベロシティ 100 のノートは 127 より小さく、ノート後は無音。
  詳細は `docs/test-reports/evidence/issue-0068/melody-analysis.txt`。
  - 注意: 初回の書き出しは `target/debug/daw` が古く（`cargo test`
    は通常バイナリを更新しない）無音だった。再ビルド後の再実行で
    上記の結果を確認したもので、実装の不具合ではない。
- GUI 証拠（Xvfb + lavapipe、実ディスプレイ未検証）:
  `docs/test-reports/evidence/issue-0068/drive.sh` で実アプリを開き、
  起動ダンプに Sampler 楽器とノート [60, 64, 72] が現れること、
  タイムラインにノートクリップが描かれることを確認した
  （`shot1-timeline.png`、`gui-stdout.txt`）。

## 独立レビュー

- ラウンド1: **CHANGES_REQUESTED**（対象 diff `f49378b..97a4f81`）
  - Major-1: `AddNote` の範囲チェック `start_ms + duration_ms` が
    i64 オーバーフローで破れる（debug では panic、release では
    検証素通り）。MCP 入力から到達可能。
  - Major-2: `SetTrackInstrument` が `root_note` の範囲（0–127）を
    検証しない。
  - Minor-1: イシュー／ADR の言う「sampler の Voice 機構の再利用」
    がされておらず、乖離の説明がない。
  - Minor-2: ソロ時のノート再生テストがない。
  - Minor-3（観察）: ノート発音ループの `pitch_cache` 参照が
    フレーム単位で `String` clone＋ハッシュを伴う（既存の波形経路と
    同じ書き方で本 diff 起因ではない）。
- 修正（コミット `6b948aa`）:
  - Major-1: 減算形 `duration_ms > clip.duration_ms - start_ms` に
    変更し、回帰テスト `add_note_rejects_overflowing_range_and_keeps_state`
    を追加（red: debug でオーバーフロー panic）。
  - Major-2: 代入前に `root_note > 127` を `InvalidRange` で弾き、
    テスト `set_track_instrument_rejects_out_of_range_root_note` を
    追加（red: 検証なしで Ok が返っていた）。
  - Minor-2: `render_mix_note_track_respects_solo` を追加（ソロ側
    330Hz のみ鳴り、非ソロ側 220Hz が無音になることを検証）。
  - Minor-1 への回答（判断の記録）: `render_mix` は純粋関数であり、
    ライブ演奏用の Voice はリアルタイム状態機械のため、そのままは
    再利用できない。再利用したのは音高シフトの方式（`pitch_shift`、
    Signalsmith）と線形補間の読み出しであり、発音ループは描画側に
    新規実装した。ADR-0016 の「描画は純粋関数として分ける」という
    前提に沿った判断として、ここに明記する。
  - Minor-3 は将来の最適化候補として記録のみ（本 PBI では扱わない）。
- ラウンド2: **APPROVE**（指摘なし。対象 diff `97a4f81..6b948aa`。
  境界値の一致と新たなオーバーフローの余地なしを確認済み）

## 受け入れ条件との対応

1. ✅ トラックにサンプラー楽器（素材とルート音高）を設定できる
   — `SetTrackInstrument`（コマンド／MCP／ダンプで確認）。
2. ✅ 楽器の差し替えで同じノート列が別の素材音で鳴る
   — `render_mix_swapping_instrument_changes_the_sound`。
3. ✅ ノートの音高に応じてピッチ変更して再生される
   — `render_mix_note_pitch_follows_instrument_root` と実バイナリ
   書き出しの測定（220 → 277 → 442Hz）。
4. ✅ WAV 書き出しでも再生と同じメロディーが書き出される
   — `export_wav_contains_melody` と実バイナリの書き出し解析。
5. ✅ 既存の波形クリップ再生が壊れていない — 既存テスト全件 green
   （ワークスペース 298 件、失敗 0）。

## 残件・申し送り

- ノートの移動・削除とピアノロール UI は #69、MIDI 読み込みは
  #70 の範囲（本 PBI では `AddNote` まで）。
- GUI 上で楽器を設定する操作は本 PBI のスコープ外（イシューの
  スコープ外明記どおり）。当面は MCP／API／プロジェクトファイル
  経由で設定する。
- ノート発音ループの `pitch_cache` 参照最適化（レビュー Minor-3）は
  将来候補。現状は既存の波形経路と同じ性能特性。
- 実ディスプレイ／実音声デバイスでの発音は未検証（従来どおり）。
