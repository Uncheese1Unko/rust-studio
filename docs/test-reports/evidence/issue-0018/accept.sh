#!/bin/bash
# Issue #18 受け入れ: DAW の GUI で ↑ キーを押し、再生ヘッド位置の
# クリップのピッチが +1 半音され、ダンプに反映されることを確認する。
set -u
BIN="$HOME/workspace/repos/rust-studio/target/debug/daw"
W=/tmp/pitch18
rm -rf "$W"; mkdir -p "$W"
Xvfb :99 -screen 0 1024x600x24 > "$W/xvfb.log" 2>&1 &
XVFB=$!
sleep 2
DISPLAY=:99 "$BIN" > "$W/gui-stdout.txt" 2>&1 &
APP=$!
cleanup() { kill $APP $XVFB 2>/dev/null; }
trap cleanup EXIT
sleep 4
DISPLAY=:99 ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$W/shot1-before.png"
# 再生ヘッドが tone-220（0-2000ms）にかかっている間に ↑ を2回押す
DISPLAY=:99 xdotool mousemove 480 300
sleep 0.2
DISPLAY=:99 xdotool keydown Up
sleep 0.6
DISPLAY=:99 xdotool keyup Up
sleep 0.3
DISPLAY=:99 xdotool keydown Up
sleep 0.6
DISPLAY=:99 xdotool keyup Up
sleep 1
grep -c "after pitch" "$W/gui-stdout.txt" || true
DISPLAY=:99 ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$W/shot2-pitch.png"
echo done
