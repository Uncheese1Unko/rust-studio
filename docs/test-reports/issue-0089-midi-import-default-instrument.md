# 試験レポート: #89 MIDI を読み込んだトラックが、楽器を割り当てるまで無音になる

対象 PBI: #89「MIDI を読み込んだトラックが、楽器を割り当てるまで無音になる」
実施日: 2026-11-25
実装コミット: `171e228`
ベース: `e38f399`

## バグの再現（修正前）

#70 の検証用ファイル（`evidence/issue-0070/project.seed.json` と `song.mid`）で CLI フローを再実行した。

- `daw --import-midi project.json song.mid` → 成功（2トラック、5ノート、BPM 150）。ただし両トラックとも `instrument: null`、`tone_id: null`。
- `daw --export-wav project.json out.wav` → 1.60 秒・44,100Hz・2ch の WAV が出るが**ピーク 0 の完全な無音**（frames=70560, peak=0 を実測）。

根本原因: `crates/studio-core/src/midi_import.rs` はノートを持つトラックを作るが楽器を一切割り当てない。`apps/daw/src/audio.rs` の `render_mix` は、実効楽器が Sampler でも Plugin でもないトラックのノートを読み飛ばす。

## 実装内容（ADR-0022）

- `crates/studio-core/src/model.rs`: `Instrument` に `Synth { waveform: SynthWaveform }` バリアントと `SynthWaveform`（Sine／Triangle／Square／Sawtooth）を追加。`Instrument::default_synth()` は三角波。ADR-0016 の「バリアント追加が拡張点」に沿う形で、既存バリアントの JSON 形式は不変。
- `crates/studio-core/src/midi_import.rs`: 取り込みは各トラックの `AddTrack` の直後に `Command::SetTrackInstrument`（`Some(Instrument::default_synth())`）を適用する。割り当ては取り込みのコマンド列の一部なので、状態ダンプに `instrument: {"type": "synth", "waveform": "triangle"}` と正直に現れ、Undo 1回で割り当てごと取り込み前に戻る（ADR-0005 の「ダンプは画面で観測できるものと一致する」を守るため、render_mix 側の暗黙フォールバックは採らなかった。理由の詳細は ADR-0022）。
- `apps/daw/src/audio.rs`: 純粋関数 `synth_sample`（周波数 = 440 × 2^((pitch−69)/12)、5ms アタック＋末尾 10ms リリース、振幅は控えめ）を追加し、`render_mix` のノートクリップ経路に Synth の発音分岐を足した。ベロシティ・クリップゲイン・フェード・チャンネル係数はサンプラー経路と同じ扱い。外部素材も事前レンダリングも要らない。
- `apps/daw/src/main.rs`: `Instrument` の網羅 match に Synth の腕を2箇所追加（音色化の基準名 `synth-<波形>`、ピアノロール見出しの表示 `instrument: synth (<波形>)`）。

## スコープ外の判断（ピアノロールで新規トラックにノートを置く場合）

現行コードで確認したところ、ピアノロールで新規トラックにノートを置く場合も同じ無音が起こる（`AddTrack` の既定は `instrument: None` で、ピアノロールにも割り当て導線が無い）。ただし、そちらは「ノートを置くジェスチャと自動割り当てを同じ履歴単位にまとめるか」「どのトラックが対象か」という GUI の操作モデルと Undo の設計判断を含むため、本 PBI には含めず、理由と推奨案を添えて **Issue #90**（blocked by #89）へ切り分けた。本 PBI の修正で「MIDI を読み込む」という入口については追加操作なしで鳴るようになる。

既に `instrument: null` で保存されたプロジェクトファイルの自動修復も対象外（開いただけでは状態を書き換えない。利用者が TONES パネルで割り当てれば従来どおり鳴る）。

## TDD の記録（red）

- core（`crates/studio-core/tests/midi_import_tests.rs` に `import_midi_assigns_a_default_instrument_to_each_track` を追加）: `SynthWaveform` の import 解決失敗と `Instrument::Synth` バリアント不在でコンパイルエラー（red）。
- daw（`apps/daw/src/audio.rs` のテストモジュールに `imported_melody_plays_without_assigning_an_instrument` と `export_wav_of_imported_midi_is_not_silent` を追加）: コンパイルは通り、実行時に失敗（red）。失敗メッセージは `imported project must not be silent: 0`（書き出しテストも同様に失敗）。

実装後に全件 green。

## 単体テスト

- ワークスペース全体: 477 件 → **480 件**（新規3件: core 1件、daw 2件）、全て通過。
- `./scripts/verify.sh`: fmt / clippy / テストすべて OK。
- 新規テストの内容: 取り込み後の両トラックへの既定シンセ割り当てとダンプ表現・Undo 原子性（core）。取り込み→ミックスで C4（261.63Hz）と C5（523.25Hz）が区間別 Goertzel で検出され、末尾が無音に戻ること。取り込み→WAV 書き出しが無音でなく C4 を含むこと（daw）。

## 実測検証（CLI フロー、駆動スクリプトで再実行可能）

駆動スクリプトと測定スクリプトは `evidence/issue-0089/`（`drive.sh`、`measure_wav.py`、`mcp_replace.py`、`measure_replace.py`）。

- 取り込み後のダンプ: Lead／Bass 両トラックとも `instrument: {"type": "synth", "waveform": "triangle"}`、`tone_id: null`。
- 書き出し WAV: 1.60 秒、**ピーク 29490**（修正前は 0）。
- 周波数測定（Goertzel、区間ごとに ×1.5 周波数を参照値として併記）:
  - 0–400ms: Lead A4 **440.00Hz = 0.1596**（参照 0.0004）、Bass A2 110.00Hz = 0.1914（参照 0.0018）
  - 400–800ms: Lead C5 523.25Hz = 0.1757（参照 0.0006）
  - 800–1500ms: Lead E5 659.25Hz = 0.1435（参照 0.0002）、Bass F2 87.31Hz = 0.1914（参照 0.0022）
  - 全ノートが音高どおりの周波数で支配的に検出された。
- 差し替え回帰: MCP の `set_track_instrument` で Lead をサンプラー（220Hz サイン素材、ルート 60）へ差し替えて書き出すと、ピーク 21937 で、0–400ms は 369.99Hz（220 × 2^(9/12) = 0.0860、シンセの 440Hz は 0.0008 に消滅）、400–800ms は 440.00Hz（0.0351、シンセの 523.25Hz は 0.0002）となり、差し替えが従来どおり効くことを確認した。Bass はシンセのまま。

## 実アプリ駆動（GUI 証拠）

Xvfb（display :96）＋ lavapipe ソフトウェアレンダリングで実アプリを駆動した。**実ディスプレイ／実 GPU での見え方と、実際の音声デバイスからの発音は未検証**（GUI 再生は書き出しと同じ `render_mix` を共有する）。

スクリーンショットは `evidence/issue-0089/shot1-playing-a.png`、`shot2-playing-b.png`、`shot3-piano-roll.png`、標準出力は `gui-stdout.txt`。

- 取り込んだプロジェクトを開くと再生が進む: 0.7 秒間隔の2枚でトランスポート帯の差分 2,514 ピクセル、タイムライン帯の差分 2,307 ピクセル（保存済み `drive-log.txt` の値）。目視でも、1枚目「BAR 1・BEAT 1、0.0 / 1.6 s、レベル −1.3 dB」→ 2枚目「BAR 1・BEAT 3、1.0 / 1.6 s、−1.7 dB」と表示が進み、レベルメーターが無音でない値を示した。再生中のクリップはテーマの再生色（ACCENT）で表示された。
- GUI 自身の状態ダンプ（stdout）でも両トラックに `"waveform": "triangle"` の楽器が載っている。
- Lead クリップをダブルクリックしてピアノロールを開くと、見出しに **「Lead — instrument: synth (triangle)」** と表示され、3ノートが見える（再生中ノートは再生色）。
- 駆動の知見: ダブルクリックは押し 0.12 秒・間隔 0.08 秒の詰めたタイミングで送る必要があった（0.25 秒押しでは判定窓を超えた）。`drive.sh` に反映済み。

## 独立レビュー

- ラウンド1: CHANGES_REQUESTED。コード自体には受け入れ条件を満たさない欠陥は無く、全受け入れ条件をレビュアーが独立に再実行して通過を確認した（書き出しピーク 29490、440Hz の Goertzel 0.1596、ワークスペース 480 件通過、fmt／clippy クリーン）。指摘は成果物側の2点: Major 1「試験レポートが存在せず、TDD 記録・受け入れ条件ごとの確認・スコープ外の理由を監査できない」、Minor 1「駆動証拠のうち CLI／MCP 区間の実行出力が証拠フォルダに保存されていない」。
  - 対応: Major 1 は本レポートを作成して対応（TDD の記録、受け入れ条件ごとの確認結果、ピアノロール側を #90 へ切り分けた理由を記載）。Minor 1 は `drive.sh` が全出力を `evidence/issue-0089/drive-log.txt` へ保存するよう直し、再実行して DRIVE OK のログを残した。
- ラウンド2: CHANGES_REQUESTED（Minor 1件のみ）。受け入れ条件はすべて独立に確認され、コードに Major 指摘なし。唯一の指摘は「本レポートの GUI 差分ピクセル数（旧ランの 2,107／2,442）が保存済み drive-log（2,514／2,307）と一致しない」という証拠同期の問題で、レビュアー明示の是正条件は「レポート数値を drive-log に合わせること」。
  - 対応: レポートの数値を drive-log の値（2,514／2,307）に合わせ、同行の目視記述もコミット済みスクリーンショット（shot1: BEAT 1・0.0s・−1.3dB → shot2: BEAT 3・1.0s・−1.7dB）の表示に揃えた。レビューは上限2ラウンドのため再レビューは行わず、レビュアーが示した是正条件どおりに直して対応完了とした（コード変更なし）。

## Definition of Done チェック

- [x] 受け入れ条件をすべて満たす（ダンプへの明示、無音でない書き出し、音高どおりの周波数、差し替え回帰、GUI 再生と見出し表示）
- [x] TDD（red を記録）
- [x] `./scripts/verify.sh` 緑
- [x] 独立レビュー（2ラウンド。round 2 の唯一の Minor はレビュアー明示の是正条件どおりレポート数値を保存証拠に同期して対応）
- [x] 実アプリ駆動で証拠を取得（Xvfb + lavapipe、実ディスプレイ未検証と明記）
- [x] 試験レポートと証拠を配置
- [ ] CI 緑（push 後に確認）
