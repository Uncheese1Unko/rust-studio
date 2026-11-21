#!/bin/bash
# Issue #10 evidence: preview syncs with playhead.
set -u
export DISPLAY=:99
BIN=/home/hatch/workspace/repos/rust-studio/target/debug/video-editor
OUT=/tmp/ve10run
mkdir -p "$OUT"
rm -f "$OUT"/*.png "$OUT"/stdout.txt

Xvfb :99 -screen 0 1024x600x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1

"$BIN" > "$OUT/stdout.txt" 2> "$OUT/stderr.txt" &
APP_PID=$!

# wait until media preparation finished (playhead starts after model())
for i in $(seq 1 60); do
  grep -q "preview textures loaded" "$OUT/stdout.txt" 2>/dev/null && break
  sleep 0.5
done
echo "loaded at $(date +%s.%N)" >> "$OUT/times.txt"

sleep 1.2
echo "shotA $(date +%s.%N)" >> "$OUT/times.txt"
ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$OUT/shotA-opening.png"

sleep 3.2
echo "shotB $(date +%s.%N)" >> "$OUT/times.txt"
ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$OUT/shotB-interview.png"

kill "$APP_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
echo "done"
