# 試験レポート： Issue #64 切り抜きの背景フレーム選択が offset_ms を加味していない

- 日付: 2026-11-22
- 実行者: エージェント（フォローアップ実施、監督: ヨーコ）
- 対象: 人物切り抜き（#22）の背景フレーム選択が旧式
  `timeline_ms - bg.start_ms` のままで、背景クリップの `offset_ms` を
  加味していない不具合。背景が分割した右クリップの場合、合成の
  背景だけが素材の頭から再生し直してしまう。
- 実装コミット: `12e9ad1`、fmt 整形 `6024e2f`
- CI: 緑 https://github.com/Uncheese1Unko/rust-studio/actions/runs/45021964803
  （#63・#65・#64 の累積ツリーに対する push の run）
- 環境: Yoko の VM（Linux、ヘッドレス）。
- テスト件数: 279 件 → 280 件（新規1件:
  video-editor `background_frame_selection_uses_clip_offset`）。
  削除・弱体化なし。

## 変更内容

- `apps/video-editor/src/main.rs`: 切り抜きの背景フレーム選択を
  `background_frame_path` 関数に切り出し（呼び出しは handle_cutout の
  1箇所のみ、読み取り専用で状態変更なし）、ソース時刻の計算を
  プレビュー経路と同じ `media::source_time_ms(bg, timeline_ms)`
  （= `offset_ms + (t - start_ms)`、範囲外は None）経由に統一した。

## 受け入れ条件ごとの結果

### AC1: 分割した右クリップを背景にした切り抜き合成で、背景が素材の続きのフレームになる — ✅

- 単体テスト `background_frame_selection_uses_clip_offset`:
  実際のコマンド層（AddClip → SplitClip）で offset 1000 の右クリップ
  （2000〜4000ms）を作り、背景フレーム選択が右クリップ先頭で
  ソース時刻 1000 のフレーム（f010）、途中で 2000 のフレーム
  （f020）を返すことを確認した。イシューの確認方法は
  「単体テストまたは実アプリの合成結果」で、本テストが前者に当たる。
- 実アプリの合成そのものは本環境では未実施: 切り抜きの実行には
  MODNet モデルと onnxruntime が必要だが、この VM にはどちらも
  置かれていない（`cutout::find_model` / `find_onnxruntime` が
  None を返す）ため、アプリ上で切り抜きジョブを開始できない。
  選択ロジック自体は上記テストで担保した。

### AC2: 既存の切り抜き動作（offset 0 の背景）が変わらない — ✅

- 同テストで、左クリップ（offset 0）の範囲では従来どおりの
  フレーム（素材内位置そのまま、f005）が選ばれること、背景の
  範囲外では従来どおり None（背景なし）になることを固定した。
- ワークスペース全テスト（280 件）と `./scripts/verify.sh` が緑で、
  既存の切り抜き関連テストにも回帰なし。

## TDD の記録

進め方: まず選択ロジックを旧式のまま関数に切り出すリファクタを行い、
既存テストが全て通る（振る舞い不変）ことを確認してからテストを書いた。

- `background_frame_selection_uses_clip_offset`:
  右クリップ先頭（timeline 2000）で期待する f010 に対し、旧式では
  素材の頭の f000 が返り失敗（red）。

式を `media::source_time_ms` 経由に直して green。`./scripts/verify.sh`
（fmt / clippy / workspace 全テスト）も緑。

## コードレビュー

- 独立 code-reviewer（イシュー番号と diff 範囲 `bb75eaa..6024e2f`
  のみを渡した）がラウンド1で **APPROVE**。指摘なし。
- レビュアーはミューテーションチェックを実施: ヘルパーを旧式に
  戻すと当該テストが f000 を返して落ちることを確認した。また
  切り出し前後の diff を全行比較し、変わったのがソース時刻の式
  だけであること（トラック絞り込みと範囲述語は同一）を確認した。
