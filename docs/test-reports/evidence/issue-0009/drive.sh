#!/bin/bash
# Issue #9 evidence: drive video-editor GUI with xdotool.
set -u
export DISPLAY=:99
BIN=/home/hatch/workspace/repos/rust-studio/target/debug/video-editor
OUT=/tmp/ve9
mkdir -p "$OUT"
rm -f "$OUT"/*.png "$OUT"/stdout.txt

Xvfb :99 -screen 0 1024x600x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1

"$BIN" > "$OUT/stdout.txt" 2> "$OUT/stderr.txt" &
APP_PID=$!
sleep 3

drag() { # x1 y1 x2 y2 : stepped drag with left button
  xdotool mousemove "$1" "$2"; sleep 0.3
  xdotool mousedown 1; sleep 0.3
  xdotool mousemove "$3" "$4"; sleep 0.5
  xdotool mouseup 1; sleep 0.5
}

# 1) Move "opening" body (200,48) -> (284,48): +84px = +1000ms
drag 200 48 284 48
# 2) Trim "interview" right edge (666,48) -> (582,48): -84px = -1000ms duration
drag 666 48 582 48
# 3) Invalid: drag "opening" (now at x204..372, grab at 288,48) far left, stepped
xdotool mousemove 288 48; sleep 0.3
xdotool mousedown 1; sleep 0.3
for X in 240 200 160 120 80 60; do xdotool mousemove "$X" 48; sleep 0.2; done
xdotool mouseup 1; sleep 0.5
# 4) Place: double-click empty V1 area (750,48), with holds the frame polling can see
xdotool mousemove 750 48; sleep 0.3
xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.1
xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.5

# 5) Trim "interview" LEFT edge -> +1000ms start, end stays 5500.
#    opening の右端 (x=324) と interview の左端 (x=330) が近いので、
#    誤認しないよう掴み位置は x=336（左端から +6px）にする。
drag 336 48 420 48

ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$OUT/after-ops.png"

kill "$APP_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
echo "done"
