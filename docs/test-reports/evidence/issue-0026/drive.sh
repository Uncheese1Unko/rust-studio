#!/bin/bash
# Issue #26 acceptance drive: DAW GUI under Xvfb + lavapipe.
# 通常時（鍵はチップ、発音中以外ピンクなし）と、キー f（+5 半音）を
# 押したままの状態（その鍵だけホットピンク、大きなピッチ表示）を撮る。
set -u
export DISPLAY=:99
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=${1:-/tmp/daw26}
mkdir -p "$OUT"
Xvfb :99 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
"$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 40); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 1.5
# フォーカス確保（空き領域を1クリック。配置はダブルクリックのみ）
xdotool mousemove 700 300 click 1
sleep 0.3
xdotool mousemove 950 530
sleep 0.3
ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/shot1-normal.png"
xdotool keydown f
sleep 2.0
ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/shot2-key-f.png"
sleep 1.0
ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/shot3-key-f-held.png"
xdotool keyup f
sleep 0.3
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
key5 = (172, 468, 206, 508)  # f キー（+5 半音）のチップ
key0 = (22, 468, 56, 508)    # a キー（ルート）のチップ
print("shot1 key5 accent:", accent_in(f"{out}/shot1-normal.png", key5))
print("shot2 key5 accent:", accent_in(f"{out}/shot2-key-f.png", key5))
print("shot2 key0 accent:", accent_in(f"{out}/shot2-key-f.png", key0))
PYEOF
