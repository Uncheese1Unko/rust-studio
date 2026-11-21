# 試験レポート: Issue #57 動画編集 GUI トラック（レイヤー）の追加・削除・リネーム

- 対象: `apps/video-editor`（edit / main）
- 前提: #55（RenameTrack コマンド）完了後に実施。操作系は DAW #56 と
  同じ意味論に揃えた
- 結果: ワークスペース全体 132 件パス（128 → 132、本 PBI で +4）

## 実施内容

- `edit.rs` に次を追加した（DAW #56 の対応物）。
  - `hit_track_header(project, x, y)`: ヘッダーセルの判定（選択用）。
  - `hit_track_add_area(project, x, y) -> Option<TrackKind>`: 最終行
    直下のヘッダー列を上下に分け、上半分が映像、下半分が音声の追加。
  - `hit_track_delete(project, x, y)`: ヘッダー右端 18px の削除帯。
  - `add_track_command(project, kind)`: 名前は種別ごとの既存数 +1 で
    映像は `V{n}`、音声は `A{n}`。
  - `RenameSession` / `RenameInput` / `RenameAction`: DAW と同じ
    インライン編集の状態機械。
- `main.rs` の配線。
  - `.event(...)` ハンドラを追加し、キー・文字イベントを
    `RenameInput` に変換。確定は `apply` 経由のみ、拒否時は編集を
    継続して理由を標準出力に残す。
  - 編集中はポインタ操作と切り抜き（K キー）を受け付けない（名前に
    K が含まれても切り抜きが誤作動しないため）。再生ヘッドと
    プレビューは進み続ける。
  - 押下エッジの処理順: 削除帯 → 追加エリア → ヘッダー（選択／
    ダブルクリックで編集開始）→ 従来のクリップ操作。ヘッダー列は
    従来 `hit_test` が Outside を返す領域で、クリップ操作と競合しない。
  - 選択中トラックは GUI の表示状態として保持し、ヘッダーを明るく
    表示する。削除時は選択を同位置または末尾へ寄せ、ドラッグを打ち切る。
  - 描画: 編集中は編集中文字列＋キャレット、ヘッダー右端に「×」、
    追加行に「+ VIDEO」「+ AUDIO」の2セル。

## テスト（TDD の記録）

先に書いて失敗を確認したテスト（video-editor バイナリ 24 → 28 件）:

- `hit_track_add_area_returns_kind` / `hit_track_delete_area` /
  `add_track_command_names_by_kind` /
  `rename_session_enter_commits_and_esc_cancels`
  - red: `error[E0425]: cannot find function 'hit_track_add_area' ...`
    （未実装のため）
  - 実装後に3件が assertion で落ちたのは、テスト側が既存の
    `sample_project()`（V1 のみ）を V1+A1 と誤認したため。テスト専用の
    2トラックプロジェクトを作るよう直して green。

green 後の結果: video-editor バイナリ 28 件、ワークスペース全体
132 件パス。`./scripts/verify.sh` は終了コード 0。

## GUI 証拠（Xvfb + lavapipe、実ディスプレイ未検証）

`docs/test-reports/evidence/issue-0057/drive.sh` で実アプリを操作した。

- 追加: 「+ VIDEO」クリック → V3、「+ AUDIO」クリック → A2 が
  行として増える（stdout の `after track add` 2件とダンプで確認）。
  ピクセル検証（4行目の帯 x=400, y=192）: 初期 (23, 23, 28) = 背景 →
  追加後 (36, 36, 41) = 行の交互色。
- リネーム: V3 のヘッダーをダブルクリック → `rename begin: track 8`、
  打鍵で SUB に変更 → ダンプに `"name": "SUB"`。
- 配置と削除: SUB 行へダブルクリックで clip-1 を配置（`placed:`）、
  V2 の「×」で削除 → 削除後ダンプから bg クリップが消えた。
- **プレビュー合成の確認**（受け入れ条件）: V2 削除後、再生ヘッドが
  SUB 行のクリップ区間（2143〜2500ms は他の映像クリップが無い）に
  入ると、stdout に
  `preview: source=demo/dropped.mp4 playhead=2164 source_time=21 frame=0`
  が出た。追加した映像トラックのクリップが、既存トラックと同じ
  `active_video_clip` の規則でプレビューのアクティブなレイヤーとして
  扱われている。
- 撮影上の注意: 起動直後はテクスチャ読み込みと lavapipe 初期化で
  画面が黒いままの期間があり、クリックが届かなかったため、
  drive.sh は描画開始をピクセルで確認してから操作する。撮影は
  `-draw_mouse 0`（カーソルを合成しない、#56 の知見）。

## コードレビュー

round 1: **APPROVE**（指摘なし）。受け入れ条件5件すべてに証跡が
対応することをレビュアーが確認した。参考所見として、プレビューの
優先順位は「トラック順で最初に当たった映像クリップ」という既存の
規則どおりで、新規トラックが無条件に最前面になるわけではない点が
挙げられた（本 PBI の対象外の既存仕様であり、証拠の区間設定も
その規則に沿っている）。

## 実装レベルの判断（ADR-0006 に基づき記録）

- 追加エリアはヘッダー列を上下半分に分けた（映像／音声）。種別ごとに
  ボタンを分ける方が、種別を後から切り替える方式より操作が一手で済む。
- 選択中トラックの表示はヘッダーの明度変更のみ（動画側にテーマの
  アクセント規則はないため、DAW のようなピンクの縁取りは持ち込まない）。
- 削除の確認ダイアログは設けない（#56 と同じ。Undo は #29 で別途扱う）。
