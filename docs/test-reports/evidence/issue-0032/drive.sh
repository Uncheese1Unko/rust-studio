#!/bin/bash
# Issue #32 acceptance drive: DAW GUI under Xvfb + lavapipe (lane A, :91).
# プロジェクトファイル共有モードで長い1クリップ（0〜9000ms）を開き、
# 再生ヘッドが内部にある間に S キーを押して分割を確認する。
# キーはアプリのポーリングが拾えるよう keydown/keyup を分けて保持し、
# 先にヘッダーをクリックしてウィンドウへフォーカスを与える（#31 の方式）。
set -u
export DISPLAY=:91
ROOT=/home/hatch/workspace/repos/rust-studio-lane-a
OUT=${1:-/tmp/daw32}
mkdir -p "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
project = {
  "name": "split drive",
  "tracks": [{
    "id": 1, "name": "A1", "kind": "audio",
    "clips": [{
      "id": 2, "name": "long-tone", "start_ms": 0, "duration_ms": 9000,
      "source": "/tmp/rust-studio-daw-media/tone220.wav",
      "offset_ms": 0, "gain": 1.0, "cutout": False,
      "pitch_semitones": 0.0, "color": None
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
shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :91 -frames:v 1 "$OUT/$1"; }
# フォーカス用にヘッダーをクリック（トラック選択も兼ねる）
xdotool mousemove 60 48; sleep 0.2
xdotool mousedown 1; sleep 0.4; xdotool mouseup 1; sleep 0.6
shot shot1-before.png
xdotool keydown s; sleep 0.5; xdotool keyup s
sleep 1
shot shot2-after.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
grep -c "after split" "$OUT/gui-stdout.txt" | sed 's/^/after split count: /'
python3 - "$OUT" <<'PYEOF'
import sys, json, re
from PIL import Image
out = sys.argv[1]
text = open(f"{out}/gui-stdout.txt").read()
idx = text.find("after split: ")
assert idx >= 0, "no after split dump"
dump, _ = json.JSONDecoder().raw_decode(text[idx + len("after split: "):])
clips = sorted(dump["tracks"][0]["clips"], key=lambda c: c["start_ms"])
assert len(clips) == 2, f"expected 2 clips after split, got {len(clips)}"
left, right = clips
assert left["start_ms"] == 0 and left["offset_ms"] == 0, left
assert right["offset_ms"] == right["start_ms"], right
assert left["duration_ms"] + right["duration_ms"] == 9000, (left, right)
split_x = int(120 + right["start_ms"] * 0.084)
print(f"split at {right['start_ms']}ms -> x={split_x}, right offset_ms={right['offset_ms']}")
EDGE = (255, 46, 136)   # ACCENT #FF2E88（再生中クリップの面）
FACE = (255, 199, 222)  # ACCENT_SOFT #FFC7DE（通常の音声クリップの面）
# クリップ面の帯（y=60）で、分割点の左側のピクセルを見る。
# 分割前: 単一クリップが再生中で面は ACCENT。
# 分割後: 左クリップは再生中でなくなり FACE、右クリップが ACCENT。
def px(path, x, y=60):
    return Image.open(path).convert("RGB").getpixel((x, y))
b_left = px(f"{out}/shot1-before.png", split_x - 20)
a_left = px(f"{out}/shot2-after.png", split_x - 20)
a_right = px(f"{out}/shot2-after.png", split_x + 20)
print("shot1 left of split:", b_left, "| shot2 left:", a_left, "| shot2 right:", a_right)
assert b_left == EDGE, "before: clip at split area must be playing (ACCENT)"
assert a_left == FACE, "after: left clip must fall back to normal face"
assert a_right == EDGE, "after: right clip must be playing (ACCENT)"
print("PIXEL CHECK OK")
PYEOF
