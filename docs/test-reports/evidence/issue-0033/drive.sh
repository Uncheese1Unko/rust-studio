#!/bin/bash
# Issue #33 acceptance drive: DAW GUI under Xvfb + lavapipe (lane A, :91).
# プロジェクトファイル共有モードで、ゲイン 0.5 / ピッチ +2 のクリップを
# 開き、Ctrl+D（複製）→ Ctrl+C（コピー）→ Ctrl+V（貼り付け）を通す。
# キーは keydown/keyup を分けて保持し、各ステップは stdout のマーカーが
# 出るまで再フォーカスしながら最大3回リトライする（#31 の方式。
# Xvfb 上では単発のキー送信が取りこぼされることがあるため）。
set -u
export DISPLAY=:91
ROOT=/home/hatch/workspace/repos/rust-studio-lane-a
OUT=${1:-/tmp/daw33}
mkdir -p "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
project = {
  "name": "duplicate drive",
  "tracks": [{
    "id": 1, "name": "A1", "kind": "audio",
    "clips": [{
      "id": 2, "name": "phrase", "start_ms": 500, "duration_ms": 2000,
      "source": "/tmp/rust-studio-daw-media/tone440.wav",
      "offset_ms": 0, "gain": 0.5, "cutout": False,
      "pitch_semitones": 2.0, "color": None
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
focus_header() {
  xdotool mousemove 60 48; sleep 0.2
  xdotool mousedown 1; sleep 0.4; xdotool mouseup 1; sleep 0.5
}
press_ctrl() { # key
  xdotool keydown ctrl; sleep 0.2
  xdotool keydown "$1"; sleep 0.5
  xdotool keyup "$1"; sleep 0.2
  xdotool keyup ctrl; sleep 0.3
}
# marker が stdout に出るまで Ctrl+key を送る（最大3回）
send_until() { # marker key
  local marker="$1" key="$2" try
  for try in 1 2 3; do
    if grep -q "$marker" "$OUT/gui-stdout.txt"; then return 0; fi
    focus_header
    press_ctrl "$key"
    sleep 0.6
  done
  grep -q "$marker" "$OUT/gui-stdout.txt"
}
focus_header
shot shot1-initial.png
send_until "after duplicate" d || { echo "FAIL: duplicate never fired"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
shot shot2-duplicated.png
send_until "copied clip" c || { echo "FAIL: copy never fired"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
send_until "after paste" v || { echo "FAIL: paste never fired"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
shot shot3-pasted.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys, json
from PIL import Image
out = sys.argv[1]
text = open(f"{out}/gui-stdout.txt").read()
def last_dump(marker):
    idx = text.rfind(marker)
    assert idx >= 0, f"no {marker} dump"
    dump, _ = json.JSONDecoder().raw_decode(text[idx + len(marker):])
    return dump
dup = last_dump("after duplicate: ")
clips = sorted(dup["tracks"][0]["clips"], key=lambda c: c["start_ms"])
assert len(clips) == 2, f"duplicate: expected 2 clips, got {len(clips)}"
first, second = clips
assert (first["start_ms"], first["duration_ms"]) == (500, 2000), first
assert second["start_ms"] == 2500, second
for key in ["gain", "pitch_semitones", "name", "source", "duration_ms"]:
    assert second[key] == first[key], f"param {key} differs: {first} vs {second}"
print("duplicate OK: second clip at 2500 with gain/pitch copied")
paste = last_dump("after paste: ")
clips = paste["tracks"][0]["clips"]
assert len(clips) == 3, f"paste: expected 3 clips, got {len(clips)}"
pasted = max(clips, key=lambda c: c["id"])
assert pasted["start_ms"] not in (500, 2500), pasted
assert pasted["gain"] == 0.5 and pasted["pitch_semitones"] == 2.0, pasted
assert pasted["duration_ms"] == 2000, pasted
print(f"paste OK: pasted clip id={pasted['id']} at {pasted['start_ms']}ms with copied params")
PANEL = (255, 253, 249)
FACES = {(255, 46, 136), (255, 199, 222)}
def px(path, x, y=60):
    return Image.open(path).convert("RGB").getpixel((x, y))
# 3000ms 地点（x=372）: 複製前はトラック行の地色、複製後はクリップ面
b = px(f"{out}/shot1-initial.png", 372)
a = px(f"{out}/shot2-duplicated.png", 372)
print("shot1 @3000ms:", b, "| shot2 @3000ms:", a)
assert b == PANEL and a in FACES
print("PIXEL CHECK OK")
PYEOF
