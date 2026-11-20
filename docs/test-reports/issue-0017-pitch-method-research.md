# 調査レポート： Issue #17 ピッチシフト／タイムストレッチ方式の選定

- 日付: 2026-11-21
- 実行者: エージェント（デリバリーループ）
- 種別: 文献・ライセンス・実装有無の調査（コード変更なし）
- 状態: **未完了（needs-human で引き継ぎ）**。聴き比べ（AC1）が未実施の
  ため推奨方式は確定させず、イシューは close しない。

## 受け入れ条件ごとの結果

### AC1: サンプル音声（声を含む）で聴き比べ、フォルマントの保持具合を記録 — ❌ 未実施（人間の聴取が必要）

- この環境（ヘッドレス VM）では音声を聴くことができないため、聴き比べは
  実施していない。音声ファイルも所見も存在しない。
- 人間が行う聴取テストの設計は本レポート末尾に用意した。最終候補2方式に
  ついて、声サンプルで ±4 / ±7 / ±12 半音のシフト結果を聴き比べる。

### AC2: 各候補のライセンスと Rust からの利用可否を表に整理 — ✅（下表）

### AC3: リアルタイム処理が可能かを方式ごとに明記 — ✅（下表）

### AC4: 推奨方式を1つ挙げ、後継 ADR の材料を揃える — △ 材料のみ（確定は聴取後）

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
