#!/bin/bash
# Issue #104 証拠ドライブ: DAW のトランスポート操作（起動時は停止、
# スペース／ボタンでの再生・一時停止、ルーラークリック＝シーク、
# ルーラードラッグ＝ループ範囲の両立、Home/End とボタンでの
# 先頭／末尾ジャンプ、リネーム編集中のスペース無効）を実アプリで
# 確認する。Xvfb(:99) + lavapipe 前提（実ディスプレイ未検証）。
#
# デモプロジェクト: 1トラック（A1）、クリップ 0..2000 と
# 2500..4500 → プロジェクト終端 4500ms。トラック行は 48px の
# ため transport_top(1) = 184 で、ボタン行は y=192..224、再生
# ボタン中心 (282,208)、先頭 (378,208)、末尾 (458,208)。
# ルーラーは y=0..24、x>=120。
# ズーム1では px_per_ms = 840/10000 = 0.084 なので x=378 は
# (378-120)/0.084 = 3071ms、ドラッグ x=252→420 は 1571..3571ms。
set -u
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=/tmp/issue104
EV=$ROOT/docs/test-reports/evidence/issue-0104
mkdir -p "$OUT"
export DISPLAY=:99
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=$ROOT/target/debug/daw
cd "$ROOT"

Xvfb :99 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1

shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$1"; }
focus_win() {
  local wid
  wid=$(xdotool search --name "daw" 2>/dev/null | head -1)
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
hold_key() { # $1=key: ポーリング方式なので長押しで送る（#31 の知見）
  xdotool keydown "$1"; sleep 0.7; xdotool keyup "$1"; sleep 0.6
}
click_at() { # $1=x $2=y: 瞬時 click は1フレーム内に収まりポーリングで
  # 取りこぼすため、押下を保持してから離す。
  xdotool mousemove "$1" "$2" mousedown 1; sleep 0.35; xdotool mouseup 1; sleep 0.5
}
same_img() { # $1 $2: 完全一致なら 0
  python3 -c "
from PIL import Image
import sys
a = Image.open('$1').convert('RGB'); b = Image.open('$2').convert('RGB')
sys.exit(0 if list(a.getdata()) == list(b.getdata()) else 1)
"
}
PASS=0; FAIL=0
check() { # $1=説明 $2=0/1
  if [ "$2" -eq 0 ]; then echo "PASS: $1"; PASS=$((PASS+1)); else echo "FAIL: $1"; FAIL=$((FAIL+1)); fi
}
LOG="$OUT/stdout.log"
: > "$LOG"

"$BIN" > "$LOG" 2>&1 &
APP_PID=$!
wait_drawn
focus_win

# 1. 起動直後は停止: 2.5秒待っても画面が変わらない
shot "$OUT/01-initial.png"
sleep 2.5
shot "$OUT/02-still-paused.png"
same_img "$OUT/01-initial.png" "$OUT/02-still-paused.png"; check "starts paused, head does not move" $?
grep -q "^transport:" "$LOG"; [ $? -ne 0 ]; check "no transport log before input" $?

# 2. スペースで再生開始 → ヘッドが進む
hold_key space
grep -q "^transport: playing" "$LOG"; check "space starts playback" $?
sleep 1.5
shot "$OUT/03-playing.png"
same_img "$OUT/02-still-paused.png" "$OUT/03-playing.png"; [ $? -ne 0 ]; check "head advanced while playing" $?

# 3. スペースで停止 → その位置で止まり続ける（メーターも含め不変）
hold_key space
grep -q "^transport: paused" "$LOG"; check "space pauses playback" $?
shot "$OUT/04-paused.png"
sleep 2.0
shot "$OUT/05-paused-still.png"
same_img "$OUT/04-paused.png" "$OUT/05-paused-still.png"; check "head and meter frozen while paused" $?

# 4. ルーラークリック = シーク（x=378 → 3071ms）
click_at 378 12
grep -q "^seek: 3071" "$LOG"; check "ruler click seeks to 3071" $?
shot "$OUT/06-after-click-seek.png"

# 5. ルーラードラッグ = ループ範囲（x=252→420 → 1571..3571）
xdotool mousemove 252 12 mousedown 1
sleep 0.3
xdotool mousemove 336 12
sleep 0.3
xdotool mousemove 420 12 mouseup 1
sleep 0.6
grep -q "^loop range: 1571..3571" "$LOG"; check "ruler drag still makes loop range" $?
shot "$OUT/07-loop-range.png"

# 6. Home / End キー
hold_key Home
grep -q "^seek: 0" "$LOG"; check "Home jumps to start" $?
hold_key End
grep -q "^seek: 4500" "$LOG"; check "End jumps to project end" $?
shot "$OUT/08-at-end.png"

# 7. ボタン: END → START → PLAY → PAUSE
click_at 458 208
N_END=$(grep -c "^seek: 4500" "$LOG")
[ "$N_END" -ge 2 ]; check "END button jumps to end" $?
click_at 378 208
N_ZERO=$(grep -c "^seek: 0" "$LOG")
[ "$N_ZERO" -ge 2 ]; check "START button jumps to start" $?
click_at 282 208
N_PLAY=$(grep -c "^transport: playing" "$LOG")
[ "$N_PLAY" -ge 2 ]; check "PLAY button starts playback" $?
shot "$OUT/09-playing-by-button.png"
click_at 282 208
N_PAUSE=$(grep -c "^transport: paused" "$LOG")
[ "$N_PAUSE" -ge 2 ]; check "PAUSE button stops playback" $?

# 8. リネーム編集中はスペースで再生が切り替わらない
# ダブルクリック判定は 400ms 窓なので、短い保持で2回押す。
xdotool mousemove 60 42 mousedown 1; sleep 0.18; xdotool mouseup 1; sleep 0.08
xdotool mousedown 1; sleep 0.18; xdotool mouseup 1
sleep 0.6
grep -q "^rename begin" "$LOG"; check "rename session started" $?
BEFORE=$(grep -c "^transport:" "$LOG")
hold_key space
AFTER=$(grep -c "^transport:" "$LOG")
[ "$BEFORE" -eq "$AFTER" ]; check "space during rename does not toggle transport" $?
shot "$OUT/10-rename-space.png"
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
cp "$LOG" "$EV/gui-stdout.txt"
echo "drive result: PASS=$PASS FAIL=$FAIL"
[ "$FAIL" -eq 0 ]
