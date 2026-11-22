#!/bin/bash
# Issue #41 acceptance drive: DAW GUI under Xvfb + lavapipe (lane A, :91)
# plus the --export-wav CLI. GUI の EXPORT ボタンを押して、初期値の
# 書き出し先に WAV ができ、中身が形式どおりで無音でないことを確認。
set -u
export DISPLAY=:91
ROOT=/home/hatch/workspace/repos/rust-studio-lane-a
OUT=${1:-/tmp/daw41}
mkdir -p "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, math, struct, sys, wave
out = sys.argv[1]
w = wave.open(f"{out}/tone.wav", "wb")
w.setnchannels(1); w.setsampwidth(2); w.setframerate(44100)
w.writeframes(b"".join(
    struct.pack("<h", int(0.4 * 32767 * math.sin(2 * math.pi * 440 * i / 44100)))
    for i in range(44100 * 2)))
w.close()
project = {
  "name": "export drive", "bpm": 120.0,
  "tracks": [{
    "id": 1, "name": "A1", "kind": "audio",
    "clips": [{"id": 2, "name": "c", "start_ms": 500, "duration_ms": 1500,
               "source": f"{out}/tone.wav",
               "offset_ms": 0, "gain": 1.0, "cutout": False,
               "pitch_semitones": 0.0, "color": None, "fade_in_ms": 0, "fade_out_ms": 0}],
    "volume": 1.0, "pan": 0.0, "color": None, "muted": False, "solo": False
  }],
  "next_id": 3
}
open(f"{out}/project.json", "w").write(json.dumps(project))
PYEOF
# CLI 経路も同じプロジェクトで確認する
"$ROOT/target/debug/daw" --export-wav "$OUT/project.json" "$OUT/cli-out.wav" > "$OUT/cli-stdout.txt" 2>&1
grep -q "exported:" "$OUT/cli-stdout.txt" || { echo "cli export failed"; exit 1; }
Xvfb :91 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE="$OUT/project.json" "$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 1
# EXPORT チップ（1トラック時: 中心 510,230）をクリック
for try in 1 2 3; do
  xdotool mousemove 510 230; sleep 0.3
  xdotool mousedown 1; sleep 0.5; xdotool mouseup 1; sleep 0.6
  grep -q "exported:" "$OUT/gui-stdout.txt" && break
done
grep -q "exported:" "$OUT/gui-stdout.txt" || { echo "gui export failed"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :91 -frames:v 1 "$OUT/shot1-exported.png"
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import struct, sys, wave
out = sys.argv[1]
for name in ("export drive.wav", "cli-out.wav"):
    w = wave.open(f"{out}/{name}")
    assert w.getframerate() == 44100 and w.getnchannels() == 2 and w.getsampwidth() == 2, name
    assert abs(w.getnframes() - 2000 * 44.1) < 2, (name, w.getnframes())
    samples = struct.unpack(f"<{w.getnframes()*2}h", w.readframes(w.getnframes()))
    peak = max(abs(s) for s in samples)
    assert peak > 1000, (name, peak)
    head = samples[: int(0.4 * 44100) * 2]
    assert max(abs(s) for s in head) == 0, (name, "head must be silent")
    print(name, "OK, peak", peak)
print("GUI + CLI EXPORT CHECK OK")
PYEOF
