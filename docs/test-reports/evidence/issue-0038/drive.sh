#!/bin/bash
# Issue #38 acceptance drive: DAW GUI under Xvfb + lavapipe (lane A, :91).
# Ctrl+ホイールでズームし、通常ホイールでスクロールする。stdout の
# zoom/scroll ログと、クリップ面のピクセル移動で確認する。表示状態は
# プロジェクト JSON に保存されないこともあわせて確認する。
set -u
export DISPLAY=:91
ROOT=/home/hatch/workspace/repos/rust-studio-lane-a
OUT=${1:-/tmp/daw38}
mkdir -p "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
project = {
  "name": "zoom drive", "bpm": 120.0,
  "tracks": [{
    "id": 1, "name": "A1", "kind": "audio",
    "clips": [{
      "id": 2, "name": "phrase", "start_ms": 2000, "duration_ms": 2000,
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
cp "$OUT/project.json" "$OUT/project-original.json"
Xvfb :91 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE="$OUT/project.json" "$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 1
shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :91 -frames:v 1 "$OUT/$1"; }
shot shot1-zoom1.png
# Ctrl+ホイール上 x2（マウスは 480,100）。X11 ではホイールボタンの
# press/release が各1イベントになるため、2クリックで4段 = 1.25^4 倍。
xdotool mousemove 480 100; sleep 0.3
xdotool keydown ctrl; sleep 0.3
xdotool click 4; sleep 0.5
xdotool click 4; sleep 0.5
xdotool keyup ctrl; sleep 0.5
shot shot2-zoomed.png
# 通常ホイール下 x3 → スクロールで offset が +300 される
xdotool click 5; sleep 0.4
xdotool click 5; sleep 0.4
xdotool click 5; sleep 0.6
shot shot3-scrolled.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys, json, re
from PIL import Image
out = sys.argv[1]
text = open(f"{out}/gui-stdout.txt").read()
zooms = re.findall(r"zoom: ([\d.]+) offset: (\d+)", text)
scrolls = re.findall(r"scroll offset: (\d+)", text)
print("zoom events:", zooms, "| scroll events:", scrolls)
assert zooms, "no zoom event"
last_zoom, last_zoom_offset = float(zooms[-1][0]), int(zooms[-1][1])
assert abs(last_zoom - 1.25 ** 4) < 0.01, last_zoom
assert last_zoom_offset > 0, last_zoom_offset
# アンカー（マウス下の時刻 4286ms）がズーム後も同じ画面位置にある
anchor_ms = last_zoom_offset + (480 - 120) / (0.084 * last_zoom)
assert abs(anchor_ms - 4286) < 5, anchor_ms
assert scrolls, "no scroll event"
final_offset = int(scrolls[-1])
# ホイール下3クリック = 6イベント x 100ms
assert final_offset == last_zoom_offset + 600, (final_offset, last_zoom_offset)
# ピクセル: x=200 は zoom 前はクリップ外（地色）、zoom 後はクリップ面
PANEL = (255, 253, 249)
FACES = {(255, 46, 136), (255, 199, 222)}
b = Image.open(f"{out}/shot1-zoom1.png").convert("RGB").getpixel((200, 60))
a = Image.open(f"{out}/shot2-zoomed.png").convert("RGB").getpixel((200, 60))
print("pixel @200,60: before", b, "| after zoom", a)
assert b == PANEL, b
assert a in FACES, a
# 表示状態はプロジェクトに保存されない（ファイルが元のまま）
orig = open(f"{out}/project-original.json").read()
now = open(f"{out}/project.json").read()
assert json.loads(now) == json.loads(orig), "project file must not gain view state"
print("ZOOM + SCROLL CHECK OK")
PYEOF
