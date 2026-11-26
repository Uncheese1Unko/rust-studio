#!/bin/bash
# Issue #101 追加駆動: 分離済みプロジェクトを GUI で開き、
# (1) ウィンドウを縦に広げて 07 TONES パネルに4音色が見えること、
# (2) 再生中に再生ヘッドが前進すること（2枚の画素比較）を確認する。
set -u
export DISPLAY=:98
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=/home/hatch/demucs-work/gui101
Xvfb :98 -screen 0 1000x1200x24 > "$OUT/xvfb2.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE="$OUT/project.json" "$ROOT/target/debug/daw" > "$OUT/gui2-stdout.txt" 2> "$OUT/gui2-stderr.txt" &
GUI_PID=$!
cleanup() { kill $GUI_PID $XVFB_PID 2>/dev/null; }
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui2-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 1
xdotool search --name "^daw$" windowsize 960 1100
sleep 2
ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 1000x1200 -i :98 -frames:v 1 "$OUT/shot4-tones-panel.png"
# 再生: Space で開始し、1.5秒間隔で2枚撮る
xdotool mousemove 480 200
xdotool key space
sleep 0.5
ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 1000x1200 -i :98 -frames:v 1 "$OUT/shot5-play-a.png"
sleep 1.5
ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 1000x1200 -i :98 -frames:v 1 "$OUT/shot6-play-b.png"
xdotool key space
cleanup
python3 - "$OUT" <<'PYEOF'
import sys
sys.path.insert(0, "/usr/lib/python3/dist-packages")
from PIL import Image
out = sys.argv[1]

def playhead_x(path):
    im = Image.open(path).convert("RGB")
    # タイムライン帯（y=40..330）で、暗い縦線（再生ヘッド）を探す。
    # 再生ヘッドは温黒 #211812 の1px線。列ごとの暗画素数を数える。
    best, best_count = None, 0
    for x in range(100, 990):
        count = sum(1 for y in range(45, 330, 3)
                    if sum(im.getpixel((x, y))) < 240)
        if count > best_count:
            best, best_count = x, count
    return best, best_count

xa, ca = playhead_x(f"{out}/shot5-play-a.png")
xb, cb = playhead_x(f"{out}/shot6-play-b.png")
print(f"playhead x: {xa} (score {ca}) -> {xb} (score {cb})")
assert xb > xa + 20, "playhead did not advance during playback"
print("PLAYBACK ADVANCE OK")
PYEOF
