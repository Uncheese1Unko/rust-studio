#!/bin/bash
# Issue #20 closure drive: final DAW state under Xvfb + lavapipe.
# 完成状態の全画面を2状態撮る: 通常再生時と、サンプラー発音中。
set -u
export DISPLAY=:99
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=${1:-/tmp/daw20}
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
ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/shot1-full-normal.png"
# フォーカス確保（空き領域を1クリック）→ キー f を保持して発音
xdotool mousemove 700 300 click 1
sleep 0.3
xdotool keydown f
sleep 2.0
ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/shot2-full-sounding.png"
xdotool keyup f
sleep 0.3
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys
from PIL import Image
out = sys.argv[1]
ACCENT = (255, 46, 136)
TEAL = (18, 181, 165)
BASE = (251, 244, 236)
def count_in(path, box, color):
    im = Image.open(path).convert("RGB").crop(box)
    return list(im.getdata()).count(color)
def px(path, x, y):
    return Image.open(path).convert("RGB").getpixel((x, y))
key5 = (172, 468, 206, 508)
meter = (228, 164, 692, 177)
for name in ["shot1-full-normal.png", "shot2-full-sounding.png"]:
    p = f"{out}/{name}"
    print(name, "bg(480,300):", px(p, 480, 300) == BASE,
          "key5 accent:", count_in(p, key5, ACCENT),
          "meter teal:", count_in(p, meter, TEAL))
PYEOF
