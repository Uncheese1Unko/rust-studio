# 試験レポート： Issue #49 隣接クリップ境界にクロスディゾルブを設定できる

- 日付: 2026-11-22
- 実行者: エージェント（デリバリーループ、並行レーンB＝動画編集 担当）
- 対象: 同一トラックで隣接するクリップ境界にクロスディゾルブを
  設定・解除でき、プレビューが境界区間で2クリップのブレンドに
  なる。方式は ADR-0014（採用済み）: 後続クリップの属性
  （transition_in_kind / transition_in_ms）＋ SetClipTransition、
  合成は CPU αブレンド＋既存テクスチャ経路。
- 実装コミット: `5964ea8` ＋ fmt `006e49c` ＋ レビュー修正
  `b89a3c1`
- CI: 緑 https://github.com/Uncheese1Unko/rust-studio/actions/runs/45012441921 （レビュー修正 `b89a3c1` 時点。実装 `006e49c` の CI も緑: runs/45012276541）
- 環境: Yoko の VM（Linux、ヘッドレス）。表示確認は Xvfb（:92）＋
  Mesa lavapipe（ソフトウェアレンダリング）。
  **実ディスプレイでの確認は未検証。**
- テスト件数: ワークスペース全体 253 件 → 260 件（新規7件:
  コア2＋レビュー修正2、media 2、MCP 1）。削除・弱体化なし。

## 受け入れ条件ごとの結果

### AC1: 隣接クリップ境界にクロスディゾルブ（既定 500ms）を設定でき、状態ダンプで観測できる — ✅

- コアに `Clip.transition_in_kind / transition_in_ms`
  （serde default で旧形式互換）と `Command::SetClipTransition`
  を追加。検証は ADR-0014 どおり種類で分ける（cross_dissolve は
  0 < duration <= クリップ長、none は duration 0 のみ＝解除で
  0 に正規化）。
- 単体テスト `set_clip_transition_validates_and_round_trips` が
  設定・永続化・旧形式読込・検証エラー時の状態不変・解除を担保。
  GUI の X キー操作でも `after transition:` ダンプで観測した。

### AC2: プレビューで境界区間の映像が2つのクリップのブレンドになり、区間外は従来どおり単一クリップになる — ✅

- `media::blend_factor`（イシュー指定テスト
  `blend_factor_is_linear_across_transition`: 窓の開始で 0、
  中点で 0.5、窓外は None）と `media::transition_blend`
  （直接隣接のときだけ成立し、前クリップは終了位置を過ぎた
  続きの素材時刻を使う。隙間がある場合は None をテストで固定）。
- 合成は `media::blend_frame_files` の CPU αブレンド
  （out = prev*(1-α) + next*α）で PNG を作り、α を 0.05 刻みに
  量子化したキャッシュ＋既存テクスチャ経路で表示する。
- 実アプリ証拠（`evidence/issue-0049/`）: 赤系 A／青系 B の
  隣接クリップで、窓前は赤 (198,39,38)、窓内 α=0.50 は
  (118,39,119)（ほぼ中間）、窓後は青 (38,39,199)。ブレンド
  キャッシュ画像自体にも中間フレームがあることを数値確認。

### AC3: GUI で境界を選んでトランジションを付け外しできる — ✅

- X キーの押下エッジで、選択中トラックの再生ヘッド位置にある
  クリップ（＝境界の後続クリップ）の先頭境界に既定 500ms の
  クロスディゾルブを設定／解除する。直接隣接する前のクリップ
  がない場合はログのみ。タイムラインでは窓の幅の帯をクリップ
  先頭に描く。
- 実アプリで設定（500ms がダンプに出る）と解除（none / 0）を
  往復確認し、設定状態のスクリーンショットを残した。

### AC4: MCP からも設定でき、ダンプで観測できる — ✅

- ツール `set_clip_transition` を公開し、MCP テスト
  `set_clip_transition_tool_applies_via_tools_call` が
  tools/call での適用・観測・検証エラー時の状態不変・解除を
  担保。tools/list の期待一覧も更新した。

## 実装上の判断（小さな判断として記録）

- 分割で生じる右クリップは transition_in を引き継がない
  （新しい境界は素の切り替わり。フェードインを引き継がない
  #34 の前例と一致）。左クリップは前の境界の属性を保持する。
  テスト `split_clip_clears_right_clip_transition_in`。
- 複製・貼り付けでも引き継がない（トランジションは特定の隣接
  境界に属する属性で、複製先の境界は別物。レビューでも妥当と
  判断された）。
- ブレンド済みフレームの生成は update で行い、view は用意済み
  テクスチャを引く（view は不変参照のため）。α の量子化で
  キャッシュファイル数を抑える。

## コードレビュー

- ラウンド1: **CHANGES_REQUESTED**（[中] 1件、[低] 2件）。
  - [中] TrimClip / SplitClip が transition_in_ms をクランプ
    せず、短くした後に ADR-0014 の不変条件（<= クリップ長）が
    既存コマンド経由で壊れる → フェードと同じ箇所にクランプを
    追加し、テスト2件（`trim_clip_clamps_transition_in_to_new_
    duration` / `split_clip_clamps_left_transition_in`）で固定
    して解消（コミット `b89a3c1`）。
  - [低] 試験レポートが diff に無い → 本レポートで充足。
  - [低・参考] 切り抜き合成がブレンドより優先される（cutout
    有効クリップの窓ではブレンドが表示されない）→ 組み合わせの
    端ケースとして本レポートに記録し、今回は扱わない。
- ラウンド2（修正 diff の差分確認）: **APPROVE**。クランプで
  ms=0 かつ kind=CrossDissolve になる経路は到達不能であること
  （TrimClip は長さ 0 を弾き、SplitClip は内点のみ）も確認
  された。

## TDD の記録

- コア: テスト2件を先に書き、未定義（TransitionKind /
  SetClipTransition / フィールド）で落ちることを確認。
  フィクスチャの長さ前提の誤り（4000 でなく 5000）を1件直した。
- media: `blend_factor_is_linear_across_transition` ほか2件を
  先に書き、未定義で落ちることを確認。
- レビュー修正: クランプのテスト2件を先に書き、500 のまま
  残ることで落ちるのを確認してから実装した。

## 完了の定義チェック

- [x] 受け入れ条件を全部、書いてある確認方法で1つずつ確かめた
- [x] TDD で実装した（red の記録は上記「TDD の記録」）
- [x] `cargo test` が 0 で終わった（`./scripts/verify.sh` 緑、260 件）
- [x] 画面で観測できる受け入れ条件は実アプリで確認し、証拠を残した。
      状態ダンプでも同じ状態が観測できる
- [x] push の前に独立した `code-reviewer` が `APPROVE` を返した
      （round 2）
- [x] この変更の commit で CI が緑（最後まで見た）
- [x] 判断があれば ADR を同じ commit で足し、関係する文書とルールを
      直した（設計は ADR-0014 の採用内容どおり。新規の設計判断なし）
- [x] 試験レポートと証拠を置いた（本ファイルと `evidence/issue-0049/`）
- [x] コメントに commit・CI の URL・レポートへのリンクを書いて閉じる
      （close コメントで実施）
