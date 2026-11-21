# 調査レポート： Issue #17 ピッチシフト／タイムストレッチ方式の選定

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ）
- 種別: 文献・ライセンス・実装有無の調査（コード変更なし）
- 状態: **完了**。人間の判断で Signalsmith Stretch を採用（ADR-0009）。
- 証拠の扱い: リポジトリは公開で、素材は第三者の音声のため、素材と処理後の WAV は
  コミットしない（`.gitignore`）。`measure.py` / `summarize.py` / `signalsmith_batch.rs`
  と測定結果（`measurements.*`）は `evidence/issue-0017/` にある。素材は `sample/` に
  置き、ffmpeg で `src/` に WAV 変換して `signalsmith_batch.rs`（`signalsmith-stretch`
  0.1.3 と `hound` を使う小さな Rust プログラム）と ffmpeg の `rubberband` フィルタで
  再生成できる。
  2026-11-21 追記: 声素材（`sample/`）を受領し、処理結果の WAV と客観測定
  （フォルマント移動率）を追加。人間の聴取で Signalsmith が最も自然と判定された。

## 受け入れ条件ごとの結果

### AC1: サンプル音声（声を含む）で聴き比べ、フォルマントの保持具合を記録 — ✅（聴取は総合所見のみ）

- 素材（人間が用意、`sample/`）: 声3本（「おめでとうございます」「さあ、いくぞ！」
  「よろしく頼むぞ」、各約1秒、44.1kHz モノラル）とピアノ1本（3.7秒）。
  再現のため 44.1kHz/16bit モノラル WAV に変換して `evidence/issue-0017/src/` に置いた。
- 処理: ±4/+7/+12/-7 半音。方式ごとの出力は `evidence/issue-0017/<方式>/` にある
  （`<素材>_<±N>st.wav`）。
  - `naive_resample_atempo`: 再生レート変更＋atempo。フォルマントが一緒に動く基準
    （SoundTouch 系の挙動の代用。SoundTouch 本体は未使用）
  - `rubberband_formant_shifted` / `rubberband_formant_preserved`（`_pitchq_quality`
    付きは pitchq=quality）: ffmpeg の `rubberband` フィルタ。
    **ffmpeg 経由は R2 エンジン固定（`engine` オプションなし）で、R3 は未検証**
  - `signalsmith_formant_shifted`（補正なし）/ `signalsmith_formant_preserved`
    （`set_formant_factor(1.0, true)`）: `signalsmith-stretch` 0.1.3 を実際にビルドして使用
    （`signalsmith_batch.rs`）。Windows では bindgen 用に libclang が必要
- 測定: `measure.py`（スペクトル包絡を対数ケプストラムで求め、元素材の包絡を
  周波数軸方向に α 倍すると出力の包絡に最も合う α を探す）。α=1.00 ならフォルマント
  不変、α＝シフト比ならフォルマントがピッチと一緒に動いた（チップマンク）。
  全結果は `measurements.md` / `measurements.json`。

声3本の |α-1| 平均（小さいほど保持）:

| 方式 | 上げ(+4,+7,+12) | 下げ(-4,-7) |
|---|---|---|
| 基準: リサンプル+WSOLA | 0.53 | 0.27 |
| Rubber Band(R2) 保持なし | 0.59 | 0.27 |
| Rubber Band(R2) 保持あり | 0.25 | 0.09 |
| Rubber Band(R2) 保持あり+pitchq=quality | 0.18 | 0.15 |
| Signalsmith 補正なし | 0.55 | 0.27 |
| Signalsmith 補正あり | **0.06** | **0.06** |

所見（測定値から言えること）:
- 「保持なし」の3行はどれも α≒シフト比で、ピッチ上げではフォルマントが一緒に動く。
  保持機能の ON/OFF で差が出ていることを確認した。
- Signalsmith（補正あり）は声3本・±全シフト量で α が 0.85〜1.15 に収まった。
- Rubber Band R2（保持あり）は +4 では保持されるが、+7 以上で崩れる素材がある
  （voice1 +12 は α=1.69、voice3 +12 は 1.77）。pitchq=quality で voice3 は改善する
  （1.20）が voice1 +12 は変わらない（1.66）。ただし R3 ではない点に注意。
- 限界: 声が約1秒と短く、測定は包絡の平均に基づく**代理指標**。F0 推定が
  オクターブ誤りを起こす箇所がある（voice1 +12 で F0 比が 1.0 台と出る等、
  測定側の問題でありシフト失敗の証拠ではない）。**聴感（不自然さ、ロボット感、
  音の濁り、子音の潰れ）は測れていない**。

聴取（人間、2026-11-21）: `naive_resample_atempo` / `rubberband_formant_preserved` /
`signalsmith_formant_preserved` を聴き比べ、**`signalsmith_formant_preserved` が
一番自然**だった。方式×素材×シフト量ごとの3段階評価は記録していない
（総合所見のみ）。測定値（上表）と向きが一致している。

### AC2: 各候補のライセンスと Rust からの利用可否を表に整理 — ✅（下表）

### AC3: リアルタイム処理が可能かを方式ごとに明記 — ✅（下表）

### AC4: 推奨方式を1つ挙げ、後継 ADR の材料を揃える — ✅ 推奨は Signalsmith Stretch（採用は人間の判断）

- 机上調査の範囲では最終候補を2方式に絞った（後述）。ただし方式選定の
  本質は声のフォルマント保持の聴感であり、聴取なしに推奨を確定させる
  ことはしない（証拠のない確定を避ける）。
- 後継 ADR の材料（判断基準・選択肢・未決事項）は本レポートに揃えた。

## 比較表

| 方式 / 実装 | 仕組み | フォルマント（性質）の保持 | ライセンス | リアルタイム | Rust からの利用 |
|---|---|---|---|---|---|
| **Rubber Band Library**（R3 / Finer エンジン） | 位相ボコーダ系（トランジェント検出＋帯域別処理） | **明示的な保持機能あり**: `OptionFormantPreserved` がフォルマント包絡を保ち、声のチップマンク化を避ける（公式の選択指針に明記） | **GPLv2 または商用ライセンス**（デュアル）。配布形態によっては商用ライセンスが必須（例: App Store 配布） | 可能。`OptionProcessRealTime` と Live 版（`RubberBandLiveShifter`）がある。高品質設定は CPU 負荷大 | FFI: `rubberband_sys` crate（C API の bindgen バインディング、Live・formant オプション含む）を確認。C++ 本体のビルド／リンクが別途必要 |
| **Signalsmith Stretch** | ハイブリッド・スペクトル方式（多方向の位相予測、Signalsmith 独自） | **保持の仕組みあり**: tonality limit（非線形周波数マップで音色を残す）とフォルマント補正／シフトの制御を持つ | **MIT**（本体・同梱 DSP ライブラリとも）。商用利用に制約なし | 可能。ストリーミング処理が前提の設計で、OBS のリアルタイム声フィルタでの採用例あり（処理遅延は数十 ms 級） | FFI: `signalsmith-stretch` crate（0.1.x、2025-09 更新、bindgen で C++ ヘッダオンリー本体に接続）を確認。純 Rust 移植は未確認 |
| **SoundTouch** | 時間領域（TDStretch: WSOLA 系）＋レート変換 | **保持機能なし**。ピッチシフトはレート変換＋時間伸縮のためフォルマントも一緒に動き、声はチップマンク化する | **LGPL-2.1**（別途ロイヤリティフリーの商用ライセンスも提供） | 可能。FIFO パイプライン型で低遅延、採用実績は広い | FFI / ラッパー: `soundtouch` crate（C++ ラッパー）、`soundtouch-ffi`（bindgen、本体 2.3.2 同梱）を確認 |
| **古典的フェーズボコーダ**（自前実装または crate 利用） | STFT の位相伝播による伸縮＋リサンプル | 基本形では**保持しない**（スペクトル包絡ごと周波数移動する）。保持には包絡の分離・再適用の追加実装が必要 | アルゴリズム自体は自由に実装可。`pitch_shift` crate のライセンスは本調査では未確認（要確認） | 可能（窓長ぶんの遅延。`pitch_shift` crate は 1024 サンプル窓のストリーミング） | 純 Rust: `pitch_shift` crate（フェーズボコーダの移植、ストリーミング API）を確認 |
| **WSOLA / TD-PSOLA**（アルゴリズム系） | 時間領域の重ね合わせ（WSOLA）／ピッチマーク同期（PSOLA） | WSOLA＋リサンプルでは保持しない。TD-PSOLA は単音の声に限ればフォルマントを保ち得るが、ピッチマーク精度に依存し、和音・混在素材には不向き | アルゴリズム（実装は自前） | WSOLA は可能。PSOLA は声向けでほぼオフライン前提 | 確立した crate は**見つからなかった**（自前実装になる） |
| （参考）単純リサンプリング | 再生速度の変更のみ | 保持しない。ADR-0007 が明示的に除外した方式 | — | 可能 | `rubato` 等。**ADR-0007 により候補外** |

## 机上調査での絞り込み（確定ではない）

- **SoundTouch は ADR-0007 の中核要件（性質の保持）を机上証拠の時点で
  満たさない**ため、最終候補から外すのが妥当と考える。
- 古典的フェーズボコーダと WSOLA/PSOLA は、保持の仕組みを自前で足す
  必要があり、手戻りリスクの観点で既製品の保持機能に劣る。
- **最終候補は Rubber Band（R3）と Signalsmith Stretch の2方式。**
  - Signalsmith Stretch: ライセンスが MIT で最も扱いやすく、リアルタイム
    前提の設計で、サンプラー演奏（PBI #19）の低遅延要件にも合う。
    フォルマント制御も持つ。弱みはトランジェント検出を持たないこと
    （打楽器系で差が出る可能性）。
  - Rubber Band: 声の品質での実績と明示的なフォルマント保持オプションが
    強み。ただし **GPLv2／商用ライセンスの選択はプロダクト判断**であり、
    配布形態の決定とセットで人間が判断する必要がある。
- どちらを推奨とするかは、声での聴き比べ（AC1）の結果をもって決める。
  聴取で差がなければ、ライセンスと統合の容易さから Signalsmith Stretch
  が有力、というのが机上の見立て（決定ではない）。

## 推奨

**Signalsmith Stretch**（`signalsmith-stretch` crate、`set_formant_factor(1.0, true)`）。
根拠: (1) 聴取で最も自然（人間）、(2) フォルマント移動率が全シフト量で最小（測定）、
(3) MIT でライセンス判断が不要、(4) リアルタイム前提の設計で #19 に合う。
懸念: C++ 本体を FFI で使う（Windows では libclang が必要）。純 Rust 移植は未確認。
Rubber Band R3 は未検証のため、劣後ではなく「検証していない」である点を ADR に明記する。
採用は人間が決定し、ADR-0009 に記録した。

## 後継 ADR（方式の ADR）の材料

- 判断基準: (1) 声のフォルマント保持の聴感（聴取テストで判定）、
  (2) ライセンスが配布形態と両立するか（特に Rubber Band の GPL／商用）、
  (3) リアルタイム遅延（サンプラー演奏 #19 の要件）、(4) CPU 負荷、
  (5) Rust 統合の手間（FFI ビルドの複雑さ）。
- 選択肢: Signalsmith Stretch（MIT・FFI crate）／ Rubber Band R3
  （GPL または商用・FFI crate）／（劣後）自前フェーズボコーダ＋包絡保持。
- 未決事項: 配布形態（GPL が許容できるか）、聴取テストの結果、
  対象を声中心とするか打楽器含む混在素材まで広げるか。

## 聴取テストの設計（人間への引き継ぎ）

1. 素材: (a) 人の声（朗読または歌、数秒）、(b) ピアノ等の持続音、
   (c) 可能なら声＋伴奏の混在素材。各 WAV を用意する。
2. 処理: 最終候補2方式で、それぞれ +4 / +7 / +12 / -4 / -7 半音の
   ピッチシフトを作る（Rubber Band は formant preserved 有効、
   Signalsmith はフォルマント補正有効の設定で）。
3. 記録: 方式×素材×シフト量ごとに「声の性質が保たれているか」を
   3段階で記録し、気づきを1行添える。本レポートへの追記、または
   後継 ADR の証拠として `docs/test-reports/evidence/issue-0017/` に
   音声と所見を置く。
4. 判定: 声の保持具合を最優先し、同等ならライセンス・運用面で決める。

## 出典（2026-11-21 閲覧）

- Signalsmith Stretch 公式: https://signalsmith-audio.co.uk/code/stretch/
- 同・設計解説: https://signalsmith-audio.co.uk/writing/2023/stretch-design/
- Rubber Band README（ライセンス）: https://hg.sr.ht/~breakfastquay/rubberband
- Rubber Band 統合指針（formant / リアルタイムの選択指針）: https://www.breakfastquay.com/rubberband/integration.html
- Rubber Band man ページ（`-F` フォルマント保持の説明）: https://manpages.debian.org/bookworm/rubberband-cli/rubberband.1.en.gz
- SoundTouch ライセンス: http://soundtouch.surina.net/license.html
- rubberband_sys（Rust FFI）: http://docs.rs/rubberband-sys/latest/rubberband_sys/
- signalsmith-stretch crate（docs.rs / crates.io 索引）: https://docs.rs/crate/signalsmith-stretch/0.1.1/source/signalsmith-stretch/README.md
- soundtouch-ffi（同梱 SoundTouch 2.3.2 のライセンス文）: https://docs.rs/crate/soundtouch-ffi/0.2.0/source/soundtouch-2_3_2/include/SoundTouch.h
- pitch_shift crate（純 Rust フェーズボコーダ）: https://docs.rs/pitch_shift/latest/pitch_shift/
