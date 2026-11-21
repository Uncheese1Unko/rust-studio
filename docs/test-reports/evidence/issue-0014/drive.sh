#!/bin/bash
# Issue #14 acceptance drive: DAW GUI under Xvfb.
# 1回の起動で「ドラッグ移動 → ダブルクリック配置」を行い、その
# stdout とスクリーンショットを証拠として残す。
# ダブルクリックは click --repeat では速すぎてアプリのポーリングが
# 拾えないため、明示的な down/up ペアで送る（レビュー指摘で修正）。
set -u
export DISPLAY=:99
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=/tmp/daw14
rm -rf "$OUT"; mkdir -p "$OUT"

Xvfb :99 -screen 0 1024x600x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
"$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 40); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 1.5
ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$OUT/shot1-initial.png"

# drag tone-440 (center ~414,48) left by 84px (=1000ms)
xdotool mousemove 414 48 mousedown 1
sleep 0.3
xdotool mousemove 372 48
sleep 0.3
xdotool mousemove 330 48
sleep 0.3
xdotool mouseup 1
sleep 0.6

# double-click empty timeline area at (700,48) to place a clip
# (explicit down/up pairs so the app's polling observes both presses)
xdotool mousemove 700 48
sleep 0.2
xdotool mousedown 1
sleep 0.15
xdotool mouseup 1
sleep 0.1
xdotool mousedown 1
sleep 0.15
xdotool mouseup 1
# 配置が stdout に出るまで待つ
for i in $(seq 1 10); do grep -q "placed:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 0.5
ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$OUT/shot2-after-ops.png"

kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
echo done
