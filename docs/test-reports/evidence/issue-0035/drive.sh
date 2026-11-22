#!/bin/bash
# Issue #35 acceptance drive: DAW GUI under Xvfb + lavapipe (lane A, :91).
# bpm=240 で開き（小節線は1000ms間隔）、トランスポートの BPM 表示を
# クリックして 120 に書き換え、小節線の間隔が変わることをピクセルで確認する。
set -u
export DISPLAY=:91
ROOT=/home/hatch/workspace/repos/rust-studio-lane-a
OUT=${1:-/tmp/daw35}
mkdir -p "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
project = {
  "name": "bpm drive", "bpm": 240.0,
  "tracks": [{
    "id": 1, "name": "A1", "kind": "audio",
    "clips": [{
      "id": 2, "name": "phrase", "start_ms": 3000, "duration_ms": 1000,
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
shot shot1-bpm240.png
# BPM 表示（中心 770,208）をクリックして入力モードへ（最大3回）
for try in 1 2 3; do
  if grep -q "bpm edit started" "$OUT/gui-stdout.txt"; then break; fi
  xdotool mousemove 770 208; sleep 0.2
  xdotool mousedown 1; sleep 0.4; xdotool mouseup 1; sleep 0.6
done
grep -q "bpm edit started" "$OUT/gui-stdout.txt" || { echo "FAIL: bpm edit never started"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
# 既存の "240" を消して "120" を入力し Enter で確定
xdotool key BackSpace; sleep 0.2
xdotool key BackSpace; sleep 0.2
xdotool key BackSpace; sleep 0.5
xdotool key 1; sleep 0.2
xdotool key 2; sleep 0.2
xdotool key 0; sleep 0.3
shot shot2-editing.png
xdotool key Return; sleep 0.8
shot shot3-bpm120.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys, json
from PIL import Image
out = sys.argv[1]
project = json.load(open(f"{out}/project.json"))
print("bpm after edit:", project["bpm"])
assert project["bpm"] == 120.0, project["bpm"]
text = open(f"{out}/gui-stdout.txt").read()
assert "after bpm:" in text, "no bpm dump"
# 小節線ピクセル: x=204 は 1000ms 地点。bpm=240 では小節線（LINE 色）、
# bpm=120 では線が無くトラック行の地色（PANEL 色）になる。
PANEL = (255, 253, 249)
def dist(px, want):
    return max(abs(a - b) for a, b in zip(px, want))
b = Image.open(f"{out}/shot1-bpm240.png").convert("RGB").getpixel((204, 40))
a = Image.open(f"{out}/shot3-bpm120.png").convert("RGB").getpixel((204, 40))
print("bar line pixel @1000ms: before", b, "| after", a)
# 線は1px幅でアンチエイリアスされるため、地色から十分離れていることで
# 線の存在を判定し、変更後は地色に戻ることを確認する。
assert dist(b, PANEL) >= 10, b
assert dist(a, PANEL) <= 2, a
print("PIXEL CHECK OK")
PYEOF
