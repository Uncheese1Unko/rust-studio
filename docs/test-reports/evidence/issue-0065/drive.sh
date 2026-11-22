#!/bin/bash
# Issue #65 verification drive: video-editor under Xvfb + lavapipe (:94).
# 1) リネーム開始 → Escape でキャンセルできる（#57 の従来動作）
# 2) 編集中でない Escape でもアプリが終了しない（バグ #65 の修正）
# 3) その後もプレビューログが進み続ける
set -u
export DISPLAY=:94
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=${1:-/tmp/video65}
mkdir -p "$OUT"
Xvfb :94 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
"$ROOT/target/debug/video-editor" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
echo "$GUI_PID" > "$OUT/gui.pid"
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :94 -frames:v 1 "$OUT/$1"; }
for i in $(seq 1 40); do
  shot probe.png
  if python3 -c "
from PIL import Image
import sys
im = Image.open('$OUT/probe.png').convert('RGB')
sys.exit(0 if im.getpixel((480, 270)) != (0, 0, 0) else 1)
"; then break; fi
  sleep 0.5
done
WID=$(xdotool search --name "^video-editor$" 2>/dev/null | head -1)
[ -n "$WID" ] && xdotool windowfocus "$WID" 2>/dev/null
sleep 0.5
alive() { kill -0 "$GUI_PID" 2>/dev/null; }
alive || { echo "FAIL: app not running at start"; kill $XVFB_PID 2>/dev/null; exit 1; }
# 1) リネーム: ヘッダーをダブルクリックして開始し、Escape でキャンセル
# （ダブルクリック判定は 400ms 以内。xdotool click --repeat は速すぎて
# アプリのフレームポーリングに拾われないため、mousedown/up を
# 0.15s 間隔で2回送る方式にする）
xdotool mousemove 60 48; sleep 0.2
xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.15
xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.8
grep -q "rename begin" "$OUT/gui-stdout.txt" || { echo "FAIL: rename never began"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
xdotool key Escape; sleep 1
grep -q "rename cancelled" "$OUT/gui-stdout.txt" || { echo "FAIL: rename not cancelled by Esc"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
alive || { echo "FAIL: app exited on rename-cancel Esc"; kill $XVFB_PID 2>/dev/null; exit 1; }
echo "rename cancel OK (app alive)"
# 2) 編集中でない Escape
BEFORE=$(grep -c "preview:" "$OUT/gui-stdout.txt" || true)
xdotool key Escape; sleep 2
alive || { echo "FAIL: app exited on plain Esc (bug #65)"; kill $XVFB_PID 2>/dev/null; exit 1; }
echo "plain Esc OK (app alive)"
# 3) プレビューが進み続ける
sleep 2
AFTER=$(grep -c "preview:" "$OUT/gui-stdout.txt" || true)
echo "preview lines: before=$BEFORE after=$AFTER"
[ "$AFTER" -gt "$BEFORE" ] || { echo "FAIL: preview stopped advancing"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
shot shot-after-escape.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
echo "DRIVE OK"
