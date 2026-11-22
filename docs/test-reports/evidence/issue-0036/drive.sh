#!/bin/bash
# Issue #36 acceptance drive: DAW GUI under Xvfb + lavapipe (lane A, :91).
# トランスポートのメトロノームチップをクリックして ON にし、チップの
# 色がピンクに変わることと stdout のログで確認する。クリック音そのものは
# ヘッドレスで聞けないため、波形は単体テストで担保する（イシューの方針）。
set -u
export DISPLAY=:91
ROOT=/home/hatch/workspace/repos/rust-studio-lane-a
OUT=${1:-/tmp/daw36}
mkdir -p "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
project = {
  "name": "metronome drive", "bpm": 120.0,
  "tracks": [{
    "id": 1, "name": "A1", "kind": "audio",
    "clips": [{
      "id": 2, "name": "phrase", "start_ms": 500, "duration_ms": 2000,
      "source": "/tmp/rust-studio-daw-media/tone440.wav",
      "offset_ms": 0, "gain": 1.0, "cutout": False,
      "pitch_semitones": 0.0, "color": None, "fade_in_ms": 0, "fade_out_ms": 0
    }],
    "volume": 1.0, "pan": 0.0, "color": None
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
shot shot1-off.png
# チップ中心 (126,230) をクリック（最大3回）
for try in 1 2 3; do
  if grep -q "metronome: on" "$OUT/gui-stdout.txt"; then break; fi
  xdotool mousemove 126 230; sleep 0.2
  xdotool mousedown 1; sleep 0.4; xdotool mouseup 1; sleep 0.6
done
grep -q "metronome: on" "$OUT/gui-stdout.txt" || { echo "FAIL: metronome never turned on"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
shot shot2-on.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys
from PIL import Image
out = sys.argv[1]
# チップ左寄り (50,230): OFF は PANEL_SUNK (243,231,217)、ON は ACCENT (255,46,136)
b = Image.open(f"{out}/shot1-off.png").convert("RGB").getpixel((50, 230))
a = Image.open(f"{out}/shot2-on.png").convert("RGB").getpixel((50, 230))
print("metronome chip pixel: before", b, "| after", a)
assert b == (243, 231, 217), b
assert a == (255, 46, 136), a
print("PIXEL CHECK OK")
PYEOF
