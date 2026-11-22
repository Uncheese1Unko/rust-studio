#!/bin/bash
# Issue #34 acceptance drive: DAW GUI under Xvfb + lavapipe (lane A, :91).
# クリップの左上コーナーを右へドラッグしてフェードインを付け、
# 保存されたプロジェクトの fade_in_ms と、波形上の傾斜線ピクセルで確認する。
set -u
export DISPLAY=:91
ROOT=/home/hatch/workspace/repos/rust-studio-lane-a
OUT=${1:-/tmp/daw34}
mkdir -p "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
project = {
  "name": "fade drive",
  "tracks": [{
    "id": 1, "name": "A1", "kind": "audio",
    "clips": [{
      "id": 2, "name": "phrase", "start_ms": 500, "duration_ms": 4000,
      "source": "/tmp/rust-studio-daw-media/tone440.wav",
      "offset_ms": 0, "gain": 1.0, "cutout": False,
      "pitch_semitones": 0.0, "color": None,
      "fade_in_ms": 0, "fade_out_ms": 0
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
fade_in_now() { python3 -c "import json;print(json.load(open('$OUT/project.json'))['tracks'][0]['clips'][0]['fade_in_ms'])" 2>/dev/null || echo 0; }
shot shot1-before.png
# 左上コーナー (164,30) を掴んで x=288（約1500ms 分）までドラッグ。
# 保存値が付くまで最大3回試す（Xvfb 上の取りこぼし対策）。
for try in 1 2 3; do
  cur=$(fade_in_now)
  if [ "$cur" -gt 1000 ] 2>/dev/null; then break; fi
  xdotool mousemove 164 30; sleep 0.3
  xdotool mousedown 1; sleep 0.4
  for x in 190 215 240 265 288; do xdotool mousemove $x 30; sleep 0.15; done
  xdotool mouseup 1; sleep 0.8
done
shot shot2-faded.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys, json
from PIL import Image
out = sys.argv[1]
project = json.load(open(f"{out}/project.json"))
clip = project["tracks"][0]["clips"][0]
fi = clip["fade_in_ms"]
print("fade_in_ms after corner drag:", fi)
assert 1000 < fi <= 2000, fi
assert clip["fade_out_ms"] == 0, clip
# 傾斜線のピクセル検証: 実測 fade_in から対角線の 1/4 点と 3/4 点を
# 計算し、その近傍に暗い線ピクセルがあることを確認する。再生ヘッドの
# 縦線は1列しかないので、2点とも一致するのは傾斜線だけ。
def dark_near(path, cx, cy):
    img = Image.open(path).convert("RGB")
    hits = []
    for dx in range(-3, 4):
        for dy in range(-3, 4):
            r, g, b = img.getpixel((cx + dx, cy + dy))
            if r < 100 and g < 100 and b < 100:
                hits.append((cx + dx, cy + dy, (r, g, b)))
    return hits
ppm = 840.0 / 10000.0
x0, x1 = 163.5, 163.5 + fi * ppm
top, bottom = 29.5, 66.5
q1 = (round(x0 + 0.25 * (x1 - x0)), round(bottom - 0.25 * (bottom - top)))
q3 = (round(x0 + 0.75 * (x1 - x0)), round(bottom - 0.75 * (bottom - top)))
print("slope probe points:", q1, q3)
b1 = dark_near(f"{out}/shot1-before.png", *q1)
b3 = dark_near(f"{out}/shot1-before.png", *q3)
a1 = dark_near(f"{out}/shot2-faded.png", *q1)
a3 = dark_near(f"{out}/shot2-faded.png", *q3)
print("before:", len(b1), len(b3), "| after:", len(a1), len(a3))
assert not (b1 and b3), "before must not have the slope"
assert a1 and a3, "slope line not found at both probe points"
print("PIXEL CHECK OK")
PYEOF
