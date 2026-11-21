# 試験レポート: Issue #30 調査 — ドラッグ＆ドロップ取り込みの可否と検証方法

- 対象: nannou 0.19（winit ベース）の OS ファイルドロップ取得可否、
  ヘッドレス検証方法、ドロップ位置情報の有無
- 種別: 調査 PBI。本体コードの変更なし（実験は使い捨ての最小アプリ）
- 結論: **パスは取得できる。ドロップ位置はイベントから取得できない。**
  後続の実装 PBI（#42 DAW / #53 動画編集）は「位置は選択中トラック＋
  再生位置で代替する」前提で受け入れ条件を書く必要がある。

## 実験方法

- 実験アプリ `dnd-probe`（証拠に `probe-main.rs` と `Cargo.toml` を
  同梱）: nannou 0.19 のウィンドウビルダーに `hovered_file` /
  `hovered_file_cancelled` / `dropped_file` コールバックを登録し、
  受けたパスと、ドロップ時点の `app.mouse` を標準出力に記録する。
  コールバックの型は nannou の原典で確認した
  （`nannou-0.19.0/src/window.rs:159` `DroppedFileFn = fn(&App, &mut Model, PathBuf)`、
  イベント列挙型にも `DroppedFile(PathBuf)` がある:
  `src/event.rs:152`）。
- ドロップの送り手: 実ファイルマネージャの代わりに、XDND
  プロトコル（version 5）を直接話す Python/ctypes スクリプト
  `xdnd_source.py`（証拠に同梱）。XdndEnter → XdndPosition →
  selection 変換（text/uri-list）→ XdndStatus → XdndDrop →
  XdndFinished の一連を Xvfb 上で実行する。対象（winit）が
  プロトコルどおり応答するため、検証は実ドロップと等価。
- 環境: Xvfb :99（960x720）+ lavapipe。実ディスプレイ・実 GPU は
  未検証（従来どおりの留保）。

## 結果（証拠: probe-stdout.txt / source-stdout.txt / probe-after-drop.png）

送信側ログ:

```
target window: 0x200001
selection served: text/uri-list
XdndStatus: accepted=True
XdndFinished received
```

実験アプリ側ログ:

```
probe ready
HOVERED: /tmp/dnd-test.wav
DROPPED: /tmp/dnd-test.wav mouse=(0, 0)
```

1. **パスの取得: できる。** ホバー時点で `hovered_file` が、ドロップで
   `dropped_file` が発火し、ファイルの絶対パスが `PathBuf` で渡る。
   複数ファイルは URI リストの件数分コールバックが来る形式。
2. **ドロップ位置: 取得できない。** `DroppedFile` が運ぶのはパス
   のみで、座標は含まれない。ドロップ時点の `app.mouse` は
   (0, 0)（初期値のまま）だった。XDND のドラッグ中は winit が
   CursorMoved をアプリに配送しないため、マウス位置はドラッグ開始
   前の値で止まる。XdndPosition の座標は winit 内部で消費され、
   nannou の公開 API には出てこない。
   - したがって「ドロップしたトラックと時刻に置く」は、この経路
     では実現できない。後続 PBI では配置先をイベント位置以外で
     決める規則が要る（候補: 選択中トラック＋再生ヘッド位置、
     または最終行への追加。#42 / #53 に本結果をコメントした）。
3. **ヘッドレス検証方法: 確立した。** 上記の XDND ソーススクリプトで
   自動再現できる（対象ウィンドウのタイトルと座標と URI を引数に
   実行し、アプリ側の標準出力で発火を確認する）。後続 PBI の証拠
   ドライブに組み込める。実ファイルマネージャ（pcmanfm）からの
   ドラッグでも同じ XDND が流れるため、実機確認は人の手元環境で
   の最終確認として残る。

## 後続 PBI への申し送り

- #42（DAW ドロップ取り込み）・#53（動画編集ドロップ取り込み）:
  実装は `.dropped_file(...)` をウィンドウ生成時に登録し、パスを
  受けてコマンド経由でクリップを配置する形になる。配置先は
  「選択中トラック＋再生ヘッド位置」を推奨する（位置が取れない
  ための代替規則。イシューにコメント済み）。
- 音声／動画の長さ取得や波形準備は既存の取り込み経路（DAW の
  `refresh_waveforms` / 動画のフレーム準備）を再利用できる。

## 完了の定義について

本 PBI は調査のため、コード変更・TDD・レビュー・実アプリ変更の
項目は対象外として扱う（#2・#17・#21 の前例どおり）。受け入れ条件
3件は上記の証拠で確認した。
