#!/bin/bash
# Issue #105 証拠ドライブ: 動画編集のトランスポート操作（起動時は
# 停止、スペース／ボタンでの再生・一時停止、ルーラークリック＝
# シーク、ドラッグ＝スクラブ、Home/End とボタンでの先頭／末尾
# ジャンプ、タイトル編集中のスペース無効、追従有効時の表示範囲外
# シーク）を実アプリで確認する。Xvfb(:99) + lavapipe 前提
# （実ディスプレイ未検証）。
#
# デモプロジェクト: 3トラック、クリップ終端の最大 8000ms が
# プロジェクト終端。ルーラーは y=0..24、ヘッダー列 x=0..120 に
# ボタン3つ（先頭 x=0..40、再生 x=40..80、末尾 x=80..120）。
# ズーム1では px_per_ms = 840/10000 = 0.084 なので x=540 は
# (540-120)/0.084 = 5000ms、x=300 は 2142ms、x=420 は 3571ms。
set -u
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=/tmp/issue105
EV=$ROOT/docs/test-reports/evidence/issue-0105
mkdir -p "$OUT"
export DISPLAY=:99
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=$ROOT/target/debug/video-editor
cd "$ROOT"

Xvfb :99 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1

shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$1"; }
focus_win() {
  local wid
  wid=$(xdotool search --name "video-editor" 2>/dev/null | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  sleep 0.3
}
wait_drawn() {
  for i in $(seq 1 40); do
    shot "$OUT/probe.png"
    python3 -c "
from PIL import Image
import sys
im = Image.open('$OUT/probe.png').convert('RGB')
sys.exit(0 if im.getpixel((480, 270)) != (0, 0, 0) else 1)
" && break
    sleep 0.5
  done
}
hold_key() { # ポーリング方式なので長押しで送る（#31 の知見）
  xdotool keydown "$1"; sleep 0.7; xdotool keyup "$1"; sleep 0.6
}
click_at() { # 瞬時 click は1フレーム内に収まりポーリングで
  # 取りこぼすため、押下を保持してから離す（#104 の知見）。
  xdotool mousemove "$1" "$2" mousedown 1; sleep 0.35; xdotool mouseup 1; sleep 0.5
}
same_img() {
  python3 -c "
from PIL import Image
import sys
a = Image.open('$1').convert('RGB'); b = Image.open('$2').convert('RGB')
sys.exit(0 if list(a.getdata()) == list(b.getdata()) else 1)
"
}
PASS=0; FAIL=0
check() {
  if [ "$2" -eq 0 ]; then echo "PASS: $1"; PASS=$((PASS+1)); else echo "FAIL: $1"; FAIL=$((FAIL+1)); fi
}
LOG="$OUT/stdout.log"
: > "$LOG"

"$BIN" > "$LOG" 2>&1 &
APP_PID=$!
wait_drawn
focus_win

# 1. 起動直後は停止: 2.5秒待っても画面が変わらず、
# プレビューのフレームも進まない
shot "$OUT/01-initial.png"
sleep 2.5
shot "$OUT/02-still-paused.png"
same_img "$OUT/01-initial.png" "$OUT/02-still-paused.png"; check "starts paused, head does not move" $?
N_PREVIEW=$(grep -c "^preview:" "$LOG")
[ "$N_PREVIEW" -le 1 ]; check "preview does not advance while paused" $?

# 2. スペースで再生開始 → ヘッドとプレビューが進む
hold_key space
grep -q "^transport: playing" "$LOG"; check "space starts playback" $?
sleep 1.5
shot "$OUT/03-playing.png"
same_img "$OUT/02-still-paused.png" "$OUT/03-playing.png"; [ $? -ne 0 ]; check "head advanced while playing" $?
N_PREVIEW=$(grep -c "^preview:" "$LOG")
[ "$N_PREVIEW" -ge 2 ]; check "preview frames advance while playing" $?

# 3. スペースで停止 → その位置で止まり続ける
hold_key space
grep -q "^transport: paused" "$LOG"; check "space pauses playback" $?
shot "$OUT/04-paused.png"
sleep 2.0
shot "$OUT/05-paused-still.png"
same_img "$OUT/04-paused.png" "$OUT/05-paused-still.png"; check "head frozen while paused" $?

# 4. ルーラークリック = シーク（x=540 → 5000ms）
click_at 540 12
grep -q "^seek: 5000" "$LOG"; check "ruler click seeks to 5000" $?
shot "$OUT/06-after-click-seek.png"

# 5. ルーラードラッグ = スクラブ（x=300 → 420 → 540）
xdotool mousemove 300 12 mousedown 1
sleep 0.4
xdotool mousemove 420 12
sleep 0.4
xdotool mousemove 540 12
sleep 0.4
xdotool mouseup 1
sleep 0.5
N_SEEK=$(grep -c "^seek:" "$LOG")
[ "$N_SEEK" -ge 3 ]; check "ruler drag scrubs (multiple seeks)" $?
grep "^seek:" "$LOG" | tail -1 | grep -q "5000"; check "scrub ends at 5000" $?
shot "$OUT/07-after-scrub.png"

# 6. Home / End キー
hold_key Home
grep -q "^seek: 0" "$LOG"; check "Home jumps to start" $?
hold_key End
grep -q "^seek: 8000" "$LOG"; check "End jumps to project end" $?
shot "$OUT/08-at-end.png"

# 7. ボタン: END → START → PLAY → PAUSE
click_at 100 12
N_END=$(grep -c "^seek: 8000" "$LOG")
[ "$N_END" -ge 2 ]; check "END button jumps to end" $?
click_at 20 12
N_ZERO=$(grep -c "^seek: 0" "$LOG")
[ "$N_ZERO" -ge 2 ]; check "START button jumps to start" $?
click_at 60 12
N_PLAY=$(grep -c "^transport: playing" "$LOG")
[ "$N_PLAY" -ge 2 ]; check "PLAY button starts playback" $?
shot "$OUT/09-playing-by-button.png"
click_at 60 12
N_PAUSE=$(grep -c "^transport: paused" "$LOG")
[ "$N_PAUSE" -ge 2 ]; check "PAUSE button stops playback" $?

# 8. タイトル編集中はスペースで再生が切り替わらない
# V1 ヘッダーをクリックして選択し、クリップ内（1000ms）へ
# シークしてから T（離しエッジ）でタイトル編集に入る。
click_at 60 42
click_at 240 12
grep -q "^seek: 1429" "$LOG"; check "seek into first clip for title test" $?
hold_key t
grep -q "^title input begin" "$LOG"; check "title session started" $?
BEFORE=$(grep -c "^transport:" "$LOG")
hold_key space
AFTER=$(grep -c "^transport:" "$LOG")
[ "$BEFORE" -eq "$AFTER" ]; check "space during title edit does not toggle transport" $?
shot "$OUT/10-title-space.png"
hold_key Escape

# 9. 追従有効のままズームして表示範囲を狭め、停止中に末尾へ
# シークすると表示（オフセット）もその位置へ追いつく
xdotool mousemove 300 100
xdotool keydown ctrl
for i in 1 2 3 4; do xdotool click 4; sleep 0.3; done
xdotool keyup ctrl
sleep 0.5
grep -q "^zoom:" "$LOG"; check "zoomed in for follow check" $?
OFF_BEFORE=$(grep "^scroll offset:" "$LOG" | tail -1)
hold_key End
OFF_AFTER=$(grep "^scroll offset:" "$LOG" | tail -1)
{ [ -n "$OFF_AFTER" ] && [ "$OFF_BEFORE" != "$OFF_AFTER" ]; }; check "follow view catches up on seek while paused" $?
shot "$OUT/11-follow-seek.png"

kill $APP_PID 2>/dev/null
kill $XVFB_PID 2>/dev/null

cp "$OUT"/*.png "$EV"/ 2>/dev/null
rm -f "$EV/probe.png"
cp "$LOG" "$EV/gui-stdout.txt"
echo "drive result: PASS=$PASS FAIL=$FAIL"
[ "$FAIL" -eq 0 ]
