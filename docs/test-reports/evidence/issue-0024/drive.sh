#!/bin/bash
# Issue #24 acceptance drive: DAW GUI under Xvfb + lavapipe.
# 再生ヘッドの進行に伴い「再生中クリップだけがホットピンクになる」
# ことを、タイミングの違う2枚のスクリーンショットで確認する。
set -u
export DISPLAY=:99
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=${1:-/tmp/daw24}
mkdir -p "$OUT"
Xvfb :99 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
"$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 40); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
xdotool mousemove 950 530
sleep 0.8
ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/shot1-playing-first.png"
sleep 2.7
ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/shot2-playing-second.png"
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys
from PIL import Image
out = sys.argv[1]
ACCENT = (255, 46, 136)       # #FF2E88
SOFT = (255, 199, 222)        # #FFC7DE
def counts(path, box):
    im = Image.open(path).convert("RGB").crop(box)
    data = list(im.getdata())
    return data.count(ACCENT), data.count(SOFT)
# tone-220: x 120..288, tone-440: x 330..498（行の内側 y 30..66）
first = (120, 30, 288, 66)
second = (330, 30, 498, 66)
for name in ["shot1-playing-first.png", "shot2-playing-second.png"]:
    a1, s1 = counts(f"{out}/{name}", first)
    a2, s2 = counts(f"{out}/{name}", second)
    print(f"{name}: tone-220 accent={a1} soft={s1} | tone-440 accent={a2} soft={s2}")
PYEOF
