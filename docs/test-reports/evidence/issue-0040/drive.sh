#!/bin/bash
# Issue #40 acceptance drive: DAW GUI under Xvfb + lavapipe (lane A, :91).
# ルーラーをドラッグしてループ範囲を作り、帯の表示・再生ヘッドの
# 巻き戻り・Esc での解除を確認する。
set -u
export DISPLAY=:91
ROOT=/home/hatch/workspace/repos/rust-studio-lane-a
OUT=${1:-/tmp/daw40}
mkdir -p "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
project = {
  "name": "loop drive", "bpm": 120.0,
  "tracks": [{
    "id": 1, "name": "A1", "kind": "audio",
    "clips": [{"id": 2, "name": "long", "start_ms": 0, "duration_ms": 8000,
               "source": "/tmp/rust-studio-daw-media/tone440.wav",
               "offset_ms": 0, "gain": 1.0, "cutout": False,
               "pitch_semitones": 0.0, "color": None, "fade_in_ms": 0, "fade_out_ms": 0}],
    "volume": 1.0, "pan": 0.0, "color": None, "muted": False, "solo": False
  }],
  "next_id": 3
}
open(f"{out}/project.json", "w").write(json.dumps(project))
PYEOF
Xvfb :91 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE="$OUT/project.json" "$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 1
shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :91 -frames:v 1 "$OUT/$1"; }
shot shot0-before.png
# ルーラーを 1000ms (x=204) から 3000ms (x=372) までドラッグ
xdotool mousemove 204 12; sleep 0.3
xdotool mousedown 1; sleep 0.4
for x in 232 260 288 316 344 372; do xdotool mousemove $x 12; sleep 0.25; done
sleep 0.4
xdotool mouseup 1; sleep 0.6
grep -q "loop range:" "$OUT/gui-stdout.txt" || { echo "loop range not set"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
shot shot1-loop-band.png
# 再生ヘッドの巻き戻りを観測: y=40 で暗い縦線（再生ヘッド）の x を追う
python3 - "$OUT" <<'PYEOF'
import subprocess, sys, time
from PIL import Image
out = sys.argv[1]
def playhead_x():
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-draw_mouse", "0",
                    "-f", "x11grab", "-video_size", "960x540", "-i", ":91",
                    "-frames:v", "1", f"{out}/poll.png"], check=True)
    im = Image.open(f"{out}/poll.png").convert("RGB")
    # 再生ヘッドはウォームブラック (33,24,18) の縦線。クリップ面は淡色。
    for x in range(121, 960):
        r, g, b = im.getpixel((x, 40))
        if r < 60 and g < 50 and b < 45:
            return x
    return None
xs = []
deadline = time.time() + 12
while time.time() < deadline:
    x = playhead_x()
    if x is not None:
        xs.append(x)
        # 範囲終端近く (>=350) を見た後に始端側 (<=250) へ戻れば巻き戻し
        if max(xs) >= 350 and xs[-1] <= 250 and xs.index(max(xs)) < len(xs) - 1:
            break
    time.sleep(0.4)
print("playhead x samples:", xs)
assert max(xs) >= 350, "playhead never approached the loop end"
assert xs[-1] <= 250, "playhead did not wrap back"
print("WRAP OBSERVED")
PYEOF
[ $? -ne 0 ] && { kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
# Esc で解除
xdotool keydown Escape; sleep 0.6; xdotool keyup Escape; sleep 0.6
grep -q "loop: cleared" "$OUT/gui-stdout.txt" || { echo "esc clear failed"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
shot shot2-cleared.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys
from PIL import Image
out = sys.argv[1]
SOFT = (255, 199, 222)     # ACCENT_SOFT
def px(shot, x, y):
    return Image.open(f"{out}/{shot}").convert("RGB").getpixel((x, y))
print("band pixel:", px("shot1-loop-band.png", 250, 4), "->", px("shot2-cleared.png", 250, 4))
assert px("shot1-loop-band.png", 250, 4) == SOFT
assert px("shot2-cleared.png", 250, 4) == px("shot0-before.png", 250, 4), "band must be gone after Esc"
print("LOOP BAND + ESC CLEAR CHECK OK")
PYEOF
