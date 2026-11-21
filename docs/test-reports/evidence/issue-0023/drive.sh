#!/bin/bash
# Issue #23 acceptance drive: DAW GUI under Xvfb + lavapipe.
# 起動して初期画面を撮り、ピクセル色がデザイン文書の配色表と
# 一致することを確認する。
set -u
export DISPLAY=:99
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=${1:-/tmp/daw23}
mkdir -p "$OUT"
Xvfb :99 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
"$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 40); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 2
xdotool mousemove 950 530
sleep 0.3
ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/shot1-theme.png"
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT/shot1-theme.png" <<'PYEOF'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
def px(x, y):
    return "#%02X%02X%02X" % im.getpixel((x, y))
checks = [
    ("background", px(700, 300), "#FBF4EC"),
    ("ruler band (sunk)", px(400, 12), "#F3E7D9"),
    ("track header (sunk)", px(60, 48), "#F3E7D9"),
    ("panel face", px(700, 48), "#FFFDF9"),
]
ok = True
for name, got, want in checks:
    mark = "OK" if got == want else "MISMATCH"
    if got != want: ok = False
    print(f"{name}: {got} (expected {want}) {mark}")
sys.exit(0 if ok else 1)
PYEOF
