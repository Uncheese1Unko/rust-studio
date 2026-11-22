# 試験レポート： Issue #33 DAW でクリップの複製とコピー＆ペーストができる

- 日付: 2026-11-22
- 実行者: エージェント（デリバリーループ、並行レーンA＝DAW 担当）
- 対象: DAW の GUI で Ctrl+D（複製）/ Ctrl+C（コピー）/ Ctrl+V
  （貼り付け）ができるようにする。コアへの新規コマンド追加なし。
- 実装コミット: `455534e`
- CI: `455534e` 緑 https://github.com/Uncheese1Unko/rust-studio/actions/runs/45001088779
- 環境: Yoko の VM（Linux、ヘッドレス）。表示確認は Xvfb（:91）＋Mesa
  lavapipe（ソフトウェアレンダリング）。**実ディスプレイでの確認は未検証。**
- テスト件数: ベースライン 188 件 → 192 件（新規4件、すべて
  `apps/daw/src/edit.rs` の単体テスト）。削除・弱体化なし。

## 受け入れ条件ごとの結果

### AC1: Ctrl+D で選択クリップが元クリップの直後に全パラメータ引き継ぎで複製される — ✅

- 対象クリップは既存のピッチ／色／分割と同じ規約で「選択中トラックの
  再生ヘッド位置にあるクリップ」。
- 実装方針どおり、新しいコアコマンドは作らず
  `edit::ClipPlacement`（AddClip＋パラメータ設定コマンド）で構成する:
  AddClip が name / duration / source を、新クリップ id への
  SetClipGain / SetClipPitch / SetClipCutout / SetClipColor が
  残りのパラメータを引き継ぐ（`apps/daw/src/edit.rs` の
  `duplicate_placement` / `follow_up_commands`）。
- テスト `duplicate_command_copies_all_clip_parameters`（イシュー指定の
  テスト名）: ゲイン 0.5・ピッチ +3・切り抜き on・色 Mint のクリップを
  複製し、実際に適用して全パラメータの一致を確認。
- 実アプリ証拠: ゲイン 0.5 / ピッチ +2 のクリップを開き Ctrl+D →
  stdout ダンプで開始 2500ms（元の終端）に同パラメータの複製を確認
  （`evidence/issue-0033/gui-stdout.txt`）。ピクセル検証: 3000ms 地点が
  トラック行の地色 (255,253,249) → クリップ面 (255,199,222) に変化。

### AC2: Ctrl+C でコピーし、再生ヘッドのあるトラックへ Ctrl+V で貼り付けられる — ✅

- クリップボードはアプリ内状態（`Model.clipboard`）で、プロジェクト
  には保存しない（イシューの方針どおり）。
- 貼り付け位置は選択中トラックの再生ヘッド位置。空のクリップボードや
  対象なしの場合は何もせずログを出す。
- 実アプリ証拠: Ctrl+C →「copied clip: phrase」、Ctrl+V → ダンプで
  4098ms 地点に同パラメータ（ゲイン 0.5 / ピッチ +2）のクリップが
  増えたことを確認（`shot3-pasted.png`）。

### AC3: プロジェクトファイル経由のダンプでも同じに観測できる — ✅

- 適用はすべて `History::apply` 経由で、適用後に
  `after duplicate:` / `after paste:` のダンプを出力し、プロジェクト
  ファイルへ保存する（証拠のダンプがそのまま観測結果）。

## 既知の制限とフォローアップ

- **`offset_ms` は引き継げない**: AddClip は offset を持てず、設定
  コマンドも存在しないため、分割したクリップ（offset_ms > 0）を
  複製すると、コピー側は素材の先頭から再生される。ゲイン等は
  引き継がれる。フォローアップとして **#62** を起票した
  （SetClipOffset 新設などの方式は #62 で決める）。
- Undo の粒度: 1回の複製は AddClip＋最大4件の設定コマンドとして
  履歴に積まれるため、完全な取り消しには複数回の Ctrl+Z が要る。
  イシューの実装方針（AddClip＋設定コマンドで構成）の帰結として
  受け入れ、レビューでも非ブロッキング所見として記録された。

## TDD の記録

- `duplicate_command_copies_all_clip_parameters` 他4件を先に書き、
  `error[E0425]: cannot find function duplicate_placement` /
  `paste_placement`（と EditKeyTracker 不在）で落ちることを確認して
  から実装した。
- キー発火は `edit::EditKeyTracker`（押下エッジのみ、編集中は消費
  して持ち越さない）で、履歴キー（#31）と同じ方式。単体テスト
  `edit_key_tracker_fires_on_ctrl_edges_only` が担保する。

## GUI ドライブの注記（正直な記録）

- Xvfb 上では単発の Ctrl+キー送信が取りこぼされることがあり
  （不発になるキーが実行ごとに移動した）、ドライブは各ステップを
  「stdout のマーカーが出るまで再フォーカスしながら最大3回
  リトライ」する方式（#31 の方式）にした。最終実行では複製・
  コピー・貼り付けすべてが発火し、ダンプとピクセルの検証が通った。

## コードレビュー

- 独立 code-reviewer（新規サブエージェント、イシュー番号と diff
  範囲 `92a7790..455534e` のみを提示）に依頼し、**ラウンド1で
  APPROVE**。ゲート（cargo test / fmt / clippy / diff --check）も
  レビュアーが自分で実行して全緑を確認。
- 非ブロッキング所見: Undo 粒度（上記「既知の制限」に記録）。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red の記録は上記「TDD の記録」）
- [x] `cargo test` が 0 で終わった（`./scripts/verify.sh` 緑、192 件）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断があれば ADR を同じ commit で足し、関係する文書とルールを
      直した（新規の設計判断なし。offset_ms の制限はフォローアップ
      #62 を起票）
- [x] 試験レポートと証拠を置いた（本ファイルと `evidence/issue-0033/`）
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
      （close コメントで実施）
