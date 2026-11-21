#!/bin/bash
# Issue #19 受け入れ: DAW の GUI で A+D+G を同時押しし、サンプラーの
# 和音（0/+4/+7）が発音状態として表示・記録されることを確認する。
set -u
BIN="$HOME/workspace/repos/rust-studio/target/debug/daw"
W=/tmp/sampler19-gui
rm -rf "$W"; mkdir -p "$W"
Xvfb :99 -screen 0 1024x600x24 > "$W/xvfb.log" 2>&1 &
XVFB=$!
sleep 2
DISPLAY=:99 "$BIN" > "$W/gui-stdout.txt" 2>&1 &
APP=$!
cleanup() { kill $APP $XVFB 2>/dev/null; }
trap cleanup EXIT
sleep 4
DISPLAY=:99 xdotool mousemove 480 300
sleep 0.2
DISPLAY=:99 xdotool keydown a
sleep 0.25
DISPLAY=:99 xdotool keydown d
sleep 0.25
DISPLAY=:99 xdotool keydown g
# 取りこぼし対策: note on が揃うまで待ち、足りなければ押し直す
for i in $(seq 1 6); do
  if grep -q "note on: +7" "$W/gui-stdout.txt"; then break; fi
  sleep 0.4
  DISPLAY=:99 xdotool keydown g
done
grep -E "note on" "$W/gui-stdout.txt"
DISPLAY=:99 ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$W/shot1-chord.png"
DISPLAY=:99 xdotool keyup a
DISPLAY=:99 xdotool keyup d
DISPLAY=:99 xdotool keyup g
sleep 1
grep -E "sampler source|note on|note off" "$W/gui-stdout.txt"
DISPLAY=:99 ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$W/shot2-released.png"
echo done
