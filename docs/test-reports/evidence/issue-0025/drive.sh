#!/bin/bash
# Issue #25 acceptance drive: DAW GUI under Xvfb + lavapipe.
# 通常時（つまみは淡色・選択中トラックの大きな現在値）と、
# 音量つまみのドラッグ中（つまみだけホットピンク、値が -6.0 dB に
# 更新される）を撮る。
set -u
export DISPLAY=:99
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=${1:-/tmp/daw25}
mkdir -p "$OUT"
Xvfb :99 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
"$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 40); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 1.5
xdotool mousemove 950 530
sleep 0.3
ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/shot1-normal.png"
# 音量つまみ（初期値 1.0 の位置 x=310, y=90）を掴んで左へドラッグ
xdotool mousemove 310 90 mousedown 1
sleep 0.3
xdotool mousemove 277 90
sleep 0.3
xdotool mousemove 245 90
sleep 0.3
# ボタンは保持したままカーソルだけ下へ逃がす（つまみが見えるように）
xdotool mousemove 245 140
sleep 0.5
ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/shot2-dragging.png"
xdotool mouseup 1
sleep 0.5
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys
from PIL import Image
out = sys.argv[1]
ACCENT = (255, 46, 136)
def accent_in(path, box):
    im = Image.open(path).convert("RGB").crop(box)
    return list(im.getdata()).count(ACCENT)
knob = (236, 78, 254, 102)  # ドラッグ後のつまみ位置（x=245 付近）
print("shot1 knob accent:", accent_in(f"{out}/shot1-normal.png", knob))
print("shot2 knob accent:", accent_in(f"{out}/shot2-dragging.png", knob))
PYEOF
