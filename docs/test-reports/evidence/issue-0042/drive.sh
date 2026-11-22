#!/bin/bash
# Issue #42 acceptance drive: DAW GUI under Xvfb + lavapipe (lane A, :91).
# #30 で確立した XDND ソースで実ドロップを送り、選択中トラック＋
# 再生ヘッド位置への配置・対応外ファイルの拒否・配置後の移動を確認。
set -u
export DISPLAY=:91
ROOT=/home/hatch/workspace/repos/rust-studio-lane-a
XDND=$ROOT/docs/test-reports/evidence/issue-0030/xdnd_source.py
OUT=${1:-/tmp/daw42}
mkdir -p "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, math, struct, sys, wave
out = sys.argv[1]
def tone(name, freq, seconds):
    w = wave.open(f"{out}/{name}", "wb")
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(44100)
    w.writeframes(b"".join(
        struct.pack("<h", int(0.4 * 32767 * math.sin(2 * math.pi * freq * i / 44100)))
        for i in range(int(44100 * seconds))))
    w.close()
tone("base.wav", 220, 2)
tone("drop-tone.wav", 440, 2)
open(f"{out}/notes.txt", "w").write("not audio")
project = {
  "name": "drop drive", "bpm": 120.0,
  "tracks": [{
    "id": 1, "name": "A1", "kind": "audio",
    "clips": [{"id": 3, "name": "base", "start_ms": 0, "duration_ms": 2000,
               "source": f"{out}/base.wav",
               "offset_ms": 0, "gain": 1.0, "cutout": False,
               "pitch_semitones": 0.0, "color": None, "fade_in_ms": 0, "fade_out_ms": 0}],
    "volume": 1.0, "pan": 0.0, "color": None, "muted": False, "solo": False
  }, {
    "id": 2, "name": "A2", "kind": "audio", "clips": [],
    "volume": 1.0, "pan": 0.0, "color": None, "muted": False, "solo": False
  }],
  "next_id": 4
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
# トラック2（空）を選択してからドロップする（配置先＝選択中トラック）
xdotool mousemove 60 96; sleep 0.3
xdotool mousedown 1; sleep 0.5; xdotool mouseup 1; sleep 0.5
# WAV をドロップ（座標は #30 調査どおりアプリには届かない）
python3 "$XDND" daw 400 200 "file://$OUT/drop-tone.wav" > "$OUT/xdnd1.txt" 2>&1
sleep 1
grep -q "dropped:" "$OUT/gui-stdout.txt" || { echo "drop did not import"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :91 -frames:v 1 "$OUT/shot1-dropped.png"
# 配置されたクリップをドラッグで動かせること（AC3）
python3 - "$OUT" <<'PYEOF'
import json, subprocess, sys, time
out = sys.argv[1]
def dropped_clip():
    p = json.load(open(f"{out}/project.json"))
    for t in p["tracks"]:
        for c in t["clips"]:
            if c["source"].endswith("drop-tone.wav"):
                return t["id"], c
    return None, None
tid, clip = dropped_clip()
assert clip is not None, "dropped clip not in saved project"
assert tid == 2, f"must land on the selected track 2, got track {tid}"
assert clip["duration_ms"] == 2000, clip
start0 = clip["start_ms"]
print("dropped clip start:", start0, "duration:", clip["duration_ms"])
# クリップ面の中心を掴んで右へ 20px ドラッグする
cx = 120 + (start0 + 1000) * 0.084
subprocess.run(["xdotool", "mousemove", str(int(cx)), "96"], check=True)
time.sleep(0.3)
subprocess.run(["xdotool", "mousedown", "1"], check=True)
time.sleep(0.4)
for dx in (4, 8, 12, 16, 20):
    subprocess.run(["xdotool", "mousemove", str(int(cx + dx)), "96"], check=True)
    time.sleep(0.25)
time.sleep(0.5)
subprocess.run(["xdotool", "mouseup", "1"], check=True)
time.sleep(0.8)
_, clip2 = dropped_clip()
print("after drag start:", clip2["start_ms"])
assert clip2["start_ms"] > start0 + 100, (start0, clip2["start_ms"])
print("DRAG AFTER DROP OK")
PYEOF
[ $? -ne 0 ] && { kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
# 対応外ファイルは拒否される（バースト判定の1秒を空ける）
sleep 2
python3 "$XDND" daw 400 200 "file://$OUT/notes.txt" > "$OUT/xdnd2.txt" 2>&1
sleep 1
grep -q "drop rejected:.*unsupported file type" "$OUT/gui-stdout.txt" || { echo "txt drop was not rejected"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
p = json.load(open(f"{out}/project.json"))
total = sum(len(t["clips"]) for t in p["tracks"])
assert total == 2, f"expected base + dropped clips only, got {total}"
print("DROP IMPORT CHECK OK")
PYEOF
