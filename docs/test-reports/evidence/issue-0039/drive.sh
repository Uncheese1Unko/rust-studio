#!/bin/bash
# Issue #39 acceptance drive: DAW GUI under Xvfb + lavapipe (lane A, :91).
# ミキサー帯の M / S ボタンをクリックし、状態ダンプとボタンの色で確認。
set -u
export DISPLAY=:91
ROOT=/home/hatch/workspace/repos/rust-studio-lane-a
OUT=${1:-/tmp/daw39}
mkdir -p "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
def track(tid, name, cid, src):
    return {"id": tid, "name": name, "kind": "audio",
            "clips": [{"id": cid, "name": "c", "start_ms": 0, "duration_ms": 2000,
                       "source": src, "offset_ms": 0, "gain": 1.0, "cutout": False,
                       "pitch_semitones": 0.0, "color": None, "fade_in_ms": 0, "fade_out_ms": 0}],
            "volume": 1.0, "pan": 0.0, "color": None, "muted": False, "solo": False}
project = {"name": "mute solo drive", "bpm": 120.0,
           "tracks": [track(1, "A1", 3, "/tmp/rust-studio-daw-media/tone440.wav"),
                      track(2, "B1", 4, "/tmp/rust-studio-daw-media/tone220.wav")],
           "next_id": 5}
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
click_until() { # x y marker
  for try in 1 2 3; do
    xdotool mousemove "$1" "$2"; sleep 0.3
    xdotool mousedown 1; sleep 0.5; xdotool mouseup 1; sleep 0.6
    grep -q "$3" "$OUT/gui-stdout.txt" && return 0
  done
  return 1
}
# トラック1の M ボタン中心 (137,186)
click_until 137 186 '"muted": true' || { echo "mute click failed"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
shot shot2-muted.png
# トラック2の S ボタン中心 (163,222)
click_until 163 222 '"solo": true' || { echo "solo click failed"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
shot shot3-solo.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys, json
from PIL import Image
out = sys.argv[1]
SUNK = (243, 231, 217)     # PANEL_SUNK #F3E7D9
ACCENT = (255, 46, 136)    # #FF2E88
TEAL = (18, 181, 165)      # #12B5A5
def px(shot, x, y):
    return Image.open(f"{out}/{shot}").convert("RGB").getpixel((x, y))
# M ボタン（トラック1）の左上内側: OFF は沈み面、ON でアクセント
print("M button:", px("shot1-off.png", 130, 179), "->", px("shot2-muted.png", 130, 179))
assert px("shot1-off.png", 130, 179) == SUNK
assert px("shot2-muted.png", 130, 179) == ACCENT
# S ボタン（トラック2）の左上内側: ON でティール
print("S button:", px("shot1-off.png", 158, 226), "->", px("shot3-solo.png", 158, 226))
assert px("shot1-off.png", 158, 226) == SUNK
assert px("shot3-solo.png", 158, 226) == TEAL
# 保存されたプロジェクトにも状態が残る
saved = json.load(open(f"{out}/project.json"))
assert saved["tracks"][0]["muted"] is True, saved["tracks"][0]
assert saved["tracks"][1]["solo"] is True, saved["tracks"][1]
print("MUTE + SOLO CHECK OK")
PYEOF
