# 試験レポート: Issue #56 DAW GUI トラックの追加・削除・リネーム

- 対象: `apps/daw`（edit / main / mixer）
- 前提: #55（RenameTrack コマンド）完了後に実施
- 結果: ワークスペース全体 128 件パス（116 → 121 → 128、本 PBI で +7）

## 実施内容

- `edit.rs` に次を追加した。
  - `hit_track_add_area(project, x, y)`: 最終トラック行直下・ヘッダー列の
    追加エリア判定。
  - `hit_track_delete(project, x, y) -> Option<usize>`: ヘッダー右端
    `DELETE_STRIP_PX = 18px` の削除帯判定。
  - `add_track_command(project)`: DAW の追加は Audio のみ。名前は既存の
    Audio トラック数 + 1 で `A{n}`。
  - `RenameSession` / `RenameInput` / `RenameAction`: リネーム編集の
    状態機械。ReceivedCharacter 由来の文字入力（制御文字は無視）、
    Backspace、Enter で `RenameTrack` コマンドを返し、Escape で取消。
- `main.rs` の配線。
  - アプリに `.event(...)` ハンドラを追加し、キー・文字イベントを
    `RenameInput` に変換する。確定は `apply` 経由でのみ行い、コアが
    拒否した場合は編集を継続して理由を標準出力に残す。
  - 編集中はポインタ操作・ピッチキー・サンプラーキーを抑止する
    （再生ヘッドとレベルメーターは進み続ける）。
  - 押下エッジの処理順: 削除帯 → 追加エリア → ヘッダー（選択／
    ダブルクリックで編集開始）。削除時は選択を同位置または末尾へ寄せ、
    削除したトラックを指すミキサーのアクティブ状態とドラッグを解除。
  - 描画: 編集中のトラックは編集中文字列＋キャレット「_」を表示。
    ヘッダー右端に「×」、追加エリアに PANEL_SUNK のセルと「+ TRACK」。
    名前は削除帯を除いた範囲の中央に置き、「×」と重ならないようにした。

## 途中で見つけた不具合と修正

- **追加エリアとミキサーパネルのレイアウト衝突**: 当初
  `mixer_top_y = ルーラー + トラック行数 × 行高` で、追加行の位置に
  ミキサーが描かれ、追加クリックが視覚的にも判定上も成立しなかった。
  落ちるテスト `mixer_starts_below_the_track_add_row` を書いてから、
  `mixer_top_y` を「トラック行＋追加行1行」の下に変更した。
- **手順上の失敗（記録）**: 実装コミット `1d2fdf1` は verify.sh の
  完了を待たずに push してしまい、CI が `cargo fmt --check` で落ちた
  （run 44930940442）。`cargo fmt` 適用コミット `1f1aca5` で解消し、
  以後の CI は緑（run 44930995041）。テスト自体の失敗ではない。

## テスト（TDD の記録）

先に書いて失敗を確認したテスト:

- edit: `hit_track_add_area` / `hit_track_delete_area` /
  `add_track_command_uses_next_audio_name` /
  `rename_session_enter_commits_and_esc_cancels` /
  `rename_session_edits_text`
  - red: `error[E0425]: cannot find function ...` /
    `error[E0433]: cannot find type RenameSession ...`（未実装のため）
- mixer: `mixer_starts_below_the_track_add_row`
  - red: assertion failed（追加行とミキサー上端が一致していたため）

green 後の結果: daw バイナリ 60 件（53 → 60。内訳は下記のレビュー対応
で追加した固定テスト1件を含む）、ワークスペース全体 128 件パス。
`./scripts/verify.sh` は終了コード 0。

## GUI 証拠（Xvfb + lavapipe、実ディスプレイ未検証）

`docs/test-reports/evidence/issue-0056/drive.sh` で実アプリを操作した。
クリックはポーリングが拾えるよう down/up を分けて送る（#14 の方式）。

- 追加: (60, 96) クリック → stdout に `after track add` と
  `"name": "A2"` を確認。
- リネーム: ヘッダーのダブルクリック → `rename begin: track 1`、
  打鍵で A1 → LEAD、Enter → `after rename` と `"name": "LEAD"` を確認。
  編集中のスクリーンショット（shot3）で「LEAD_」表示を確認。
- 削除: (111, 96) の「×」クリック → `after track remove` を確認。
- ピクセル検証（2行目の帯 x=400, y=96）:
  - shot1 初期: (251, 244, 236) = 背景（追加行）✓
  - shot2 追加後: (255, 253, 249) = パネル（トラック行）✓
  - shot5 削除後: (251, 244, 236) = 背景に戻る ✓

## コードレビュー

round 1: REQUEST_CHANGES（Major 2件、Minor 1件）。いずれも実装側で
実コードと原典を確認し、次のとおり対応した。round 2 で再検証を受ける。

1. **Major: トラック削除後にミキサーの操作対象がずれる**（指摘は
   「操作中スライダーがインデックス保持のため、削除で対象がずれる」）
   - 実コードでは該当しなかった。`Model::mixer_active` は
     `(u64, mixer::Param)` ＝トラック ID で保持し（main.rs）、削除時は
     ID 一致の場合のみ解除する。ドラッグ中の値解決
     `mixer::hit_with_x` も ID から現在位置を引き直す。
   - 振る舞いを固定するテスト `hit_with_x_follows_track_id_after_removal`
     を追加した（手前のトラックを削除しても対象 ID と値が変わらない、
     削除済み ID は解決できない）。
2. **Major: ダブルクリック判定の時刻源が app.time（dt 累積）で
   負荷時に誤判定する**
   - 前提が原典と異なった。nannou 0.19 の `app.time` は起動時
     `Instant` からの実経過秒（`duration.since_start`）であり、dt の
     累積ではない（nannou app.rs で確認）。動画編集側も同じ方式で、
     コードベース内で一貫している。
   - 誤解を避けるため、`edit::ClickTracker` の doc に時刻源の前提を
     明記した。
3. **Minor: 証拠スクリーンショットで編集テキストと「×」が重なる／
   選択行がピンクに見えない**
   - 重なりの正体はアプリの描画ではなく、ffmpeg x11grab が既定で
     合成する X のマウスカーソル（クリック地点に太い × 形で写る）だった。
     `drive.sh` の撮影に `-draw_mouse 0` を追加して撮り直し、塊が
     消えることをピクセルで確認した（ヘッダーの暗ピクセル 117 → 15）。
   - 選択の強調は #25 以来「行の縁取り」が仕様であり、shot2 でも
     A2 行の縁取りで示されている（塗りつぶしではない）。

round 2: **APPROVE**（新規指摘なし）。round 1 の2件はレビュアーが
実コードと nannou 原典を再確認し、指摘1は不成立（ID 保持＋
`hit_with_x_follows_track_id_after_removal` が振る舞いを固定）、
指摘2は前提誤り（`app.time` は実経過秒）と結論づけた。Minor 1 は
x11grab のカーソル合成が原因と特定し、`-draw_mouse 0` で撮り直した
証拠で解消を確認した。

## 実装レベルの判断（ADR-0006 に基づき記録）

- ダブルクリックの間隔は 400ms（ClickTracker の既定踏襲）。
- 追加トラック名は `A{n}`（n = 既存 Audio 数 + 1）。重複名はコア側で
  許容されるため、削除→追加で同名が再出現しうる（許容とする）。
- 削除の確認ダイアログは設けない（イシューのスコープどおり即時削除。
  Undo は #29 で別途扱う）。
