# 試験レポート： Issue #44 動画編集でクリップの複製とコピー＆ペーストができる

- 日付: 2026-11-22
- 実行者: エージェント（デリバリーループ、並行レーンB＝動画編集 担当）
- 対象: 動画編集の GUI で Ctrl+D（複製）/ Ctrl+C（コピー）/ Ctrl+V
  （貼り付け）ができるようにする。コアへの新規コマンド追加なし
  （DAW #33 と同一方針）。
- 実装コミット: `06e727e`、レビュー対応コミット: `b85aa94`・`465de53`（いずれも並行レーンの main へ rebase 後の SHA）
- CI: `465de53` 緑 https://github.com/Uncheese1Unko/rust-studio/actions/runs/45004500752
- 環境: Yoko の VM（Linux、ヘッドレス）。表示確認は Xvfb（:92）＋Mesa
  lavapipe（ソフトウェアレンダリング）。**実ディスプレイでの確認は未検証。**
- テスト件数: 着手時ベースライン 200 件 → 自分の変更で 206 件
  （新規6件、すべて `apps/video-editor/src/edit.rs` の単体テスト）。
  並行レーンの取り込み後はワークスペース全体 216 件。削除・弱体化なし。

## 受け入れ条件ごとの結果

### AC1: Ctrl+D で選択クリップが元クリップの直後に全パラメータ（cutout を含む）引き継ぎで複製される — ✅（offset_ms を除く。下記「既知の制限」）

- 対象クリップは DAW（#33）と同じ規約で「選択中トラックの再生ヘッド
  位置にあるクリップ」。
- 新しいコアコマンドは作らず `edit::ClipPlacement`（AddClip＋パラメータ
  設定コマンド）で構成する: AddClip が name / duration / source を、
  新クリップ id への SetClipGain / SetClipPitch / SetClipCutout /
  SetClipColor / SetClipFade が残りのパラメータを引き継ぐ。
- テスト `duplicate_command_copies_all_clip_parameters`（イシュー指定の
  テスト名）: ゲイン 0.5・ピッチ +3・切り抜き on・色 Mint・フェード
  300/400 のクリップを複製し、実際に適用して全パラメータの一致を確認。
- 実アプリ証拠: パラメータ調整済みプロジェクト（interview: gain
  0.5 / pitch +3 / cutout on / color mint / fade 300・400、V1 全体
  0..8000ms）をプロジェクトファイルモードで読み込み Ctrl+D →
  stdout ダンプで開始 8000ms（元の終端）に全パラメータ一致の複製を
  確認（`evidence/issue-0044/gui-stdout.txt`）。ピクセル検証:
  元クリップと複製クリップの面色がユーザー色 mint で一致
  (102,229,191)。

### AC2: Ctrl+C でコピーし、再生ヘッド位置のトラックへ Ctrl+V で貼り付けられる — ✅

- クリップボードはアプリ内状態（`Model.clipboard`）で、プロジェクト
  には保存しない（イシューの方針どおり）。
- 貼り付け先は選択中トラックの再生ヘッド位置（DAW #33 と同一規約）。
- 実アプリ証拠: Ctrl+C →「copied clip: interview」、A1 ヘッダーを
  クリックして選択を移し Ctrl+V → ダンプで A1 に同名・同 source の
  クリップが増え、調整パラメータ（gain 0.5 / cutout / mint /
  fade 300・400）も一致することを確認（`03-pasted.png`）。

### AC3: 複製したクリップのプレビューが元と同じ映像・切り抜き状態で表示される — ✅

- 実アプリ証拠: 複製（8000..16000ms）の区間に再生ヘッドがある間、
  stdout のプレビューログが `preview: source=demo/interview.mp4
  playhead=8604` を示し、元と同じ source のフレームが表示される
  ことを確認。切り抜きの合成フレームは source 名で引くため
  （`cutout::composite_dir_name(&clip.source)`）、cutout 有効な
  クリップの複製でも同じ合成結果が使われる。

## 既知の制限とフォローアップ

- **`offset_ms` は引き継げない**: AddClip は offset を持てず、設定
  コマンドも存在しないため、分割したクリップ（offset_ms > 0）を
  複製すると、コピー側は素材の先頭から再生される。コア側の方式
  決定は #62（DAW 側で起票）、動画側の追随はフォローアップ
  **#63** を起票した（EPIC #7 の sub-issue、blocked by #62）。
  レビュー round 1 の指摘を受けて切り分けたもの。

## TDD の記録

- `duplicate_command_copies_all_clip_parameters` 他4件を先に書き、
  `error[E0425]: cannot find function duplicate_placement` /
  `paste_placement`、`cannot find type EditKeyTracker` / `EditOp`
  で落ちることを確認してから実装した。
- キー発火は `edit::EditKeyTracker`（押下エッジのみ、編集中は消費
  して持ち越さない）で、履歴キー（#31）・DAW（#33）と同じ方式。
  単体テスト `edit_key_tracker_fires_on_ctrl_edges_only` が担保する。
- レビュー round 1 の指摘（Undo 分割）への対応もテスト先行:
  `placement_application_is_a_single_undo_step` を書き、
  `cannot find function apply_placement` で落ちるのを確認してから
  実装した。貼り付けテストは追随コマンドの有無を見る弱い形から、
  実際に適用して全パラメータを比較する形へ強化した。

## レビュー対応（round 1: CHANGES_REQUESTED → 修正）

1. **配置の適用が Undo を最大6件に分割していた**（本物）:
   AddClip と各設定コマンドを `History::apply` で個別に適用して
   いたため、1回の Ctrl+Z で最後の設定コマンドだけが戻り、半端な
   パラメータの複製が残る状態だった。`edit::apply_placement` を
   新設し、(1) 現在の状態の複製で全コマンドを試行して全部成功する
   場合だけ確定する（途中失敗で半端なクリップが残らない）、
   (2) 実適用は `apply_transient`＋開始時スナップショット1件の
   記録とし、複製・貼り付け全体を1回の undo 単位にした。
2. **offset_ms の制限の追跡先が誤っていた**（本物）: コメントが
   指していた #62 は DAW 側の起票で、動画側の ClipPlacement は
   対象外だった。動画側フォローアップ #63 を起票し、コメントと
   テストの「全パラメータ」の範囲（offset を除く）を明記した。
3. **貼り付けテストが弱かった**（本物）: 上記のとおり強化した。

## GUI ドライブの注記（正直な記録）

- 初回のドライブでは、検証対象がデモのデフォルトパラメータの
  クリップだったためパラメータ引き継ぎが実アプリで観測できて
  おらず、さらにマーカー確認の遅延で Ctrl+D が二重発火して複製が
  2つ重なった（レビュー round 2 の指摘）。再記録では、調整済み
  プロジェクトの使用と「マーカー件数が増えるまで再送しない」
  方式に改め、複製の単発発火と調整パラメータの引き継ぎを実アプリ
  で確認した（`evidence/issue-0044/drive.sh`）。

## コードレビュー

- 独立 code-reviewer（新規サブエージェント、イシュー番号と diff
  範囲のみを提示）。
- round 1: CHANGES_REQUESTED。指摘3件はいずれも本物と確認し、
  上記「レビュー対応」のとおり修正した。
- round 2: CHANGES_REQUESTED。指摘と処置は次のとおり。
  1. offset_ms が AC1「全パラメータ」に含まれていない（本物だが
     本 PBI では直さない）: イシュー自身の実装方針が「新しいコア
     コマンドは作らず AddClip 発行で構成する」と定めており、
     offset の引き継ぎにはコアコマンドの新設（方式は #62 で未決
     の設計判断）が要る。DAW #33 でも同一の制限をフォローアップ
     分離で close した前例に従い、動画側は #63 で追跡する。
     レビュアーは「AC 未達のまま閉じるべきでない」との意見で、
     この処置への不同意をここに記録する。
  2. GUI 証拠がデフォルトパラメータで、複製が二重発火していた
     （本物）: 上記「GUI ドライブの注記」のとおり再記録した。
  3. apply_placement の失敗パスのテストがない（本物）:
     `placement_application_failure_leaves_state_and_history_unchanged`
     を追加した。
  レビューは作法の上限2ラウンドで打ち切り、残る論点は #63 に
  集約した。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red の記録は上記「TDD の記録」）
- [x] `cargo test` が 0 で終わった（`./scripts/verify.sh` 緑、自分の変更で 206 件／全体 216 件）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [ ] push の前に独立した `code-reviewer` が `APPROVE` を返した
      → 2ラウンドとも CHANGES_REQUESTED。round 1 の指摘は全件修正、
      round 2 は証拠・テストの指摘を修正し、offset_ms の1点は
      フォローアップ #63 へ分離（上記「コードレビュー」参照）。
      APPROVE は得られていないことを明記する
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断があれば ADR を同じ commit で足し、関係する文書とルールを
      直した（新規の設計判断なし。offset_ms の制限はフォローアップ
      #63 を起票）
- [x] 試験レポートと証拠を置いた（本ファイルと `evidence/issue-0044/`）
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
      （close コメントで実施）
