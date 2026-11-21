#!/bin/bash
# Issue #27 acceptance drive: DAW GUI under Xvfb + lavapipe.
# 通常再生（メーターはティール）と、音量を最大まで上げた状態
# （ピーク -3dB 以上でメーターがホットピンク）を撮る。
set -u
export DISPLAY=:99
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=${1:-/tmp/daw27}
mkdir -p "$OUT"
Xvfb :99 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
"$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 40); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 0.6
xdotool mousemove 950 530
sleep 0.3
ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/shot1-normal.png"
# 音量つまみ（初期値 1.0 の位置 x=310, y=90）を右端（音量 2.0）へ
xdotool mousemove 310 90 mousedown 1
sleep 0.3
xdotool mousemove 375 90
sleep 0.3
xdotool mousemove 440 90
sleep 0.3
xdotool mousemove 440 140
sleep 0.8
ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/shot2-peak.png"
xdotool mouseup 1
sleep 0.3
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys
from PIL import Image
out = sys.argv[1]
ACCENT = (255, 46, 136)
TEAL = (18, 181, 165)
def count_in(path, box, color):
    im = Image.open(path).convert("RGB").crop(box)
    return list(im.getdata()).count(color)
meter = (228, 164, 692, 177)     # レベルメーターのバー領域
progress = (228, 147, 692, 160)  # 進行バーの領域
for name in ["shot1-normal.png", "shot2-peak.png"]:
    p = f"{out}/{name}"
    print(name, "meter teal:", count_in(p, meter, TEAL),
          "meter accent:", count_in(p, meter, ACCENT),
          "progress accent:", count_in(p, progress, ACCENT))
PYEOF
