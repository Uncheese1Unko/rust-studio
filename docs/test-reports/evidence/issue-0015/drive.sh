#!/bin/bash
set -u
export DISPLAY=:99
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=/tmp/daw15
rm -rf "$OUT"; mkdir -p "$OUT"
Xvfb :99 -screen 0 1024x600x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
"$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 40); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 1.5
ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$OUT/shot1-mixer.png"
# volume knob (310,90) -> (245,90): volume 1.0 -> 0.5
xdotool mousemove 310 90 mousedown 1; sleep 0.3
xdotool mousemove 245 90; sleep 0.3
xdotool mouseup 1; sleep 0.5
# pan knob (650,90) -> (715,90): pan 0 -> +0.5
xdotool mousemove 650 90 mousedown 1; sleep 0.3
xdotool mousemove 715 90; sleep 0.3
xdotool mouseup 1; sleep 0.5
ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$OUT/shot2-mixer-after.png"
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
echo done
