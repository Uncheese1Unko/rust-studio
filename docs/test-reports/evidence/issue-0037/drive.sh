#!/bin/bash
# Issue #37 acceptance drive: DAW GUI under Xvfb + lavapipe (lane A, :91).
# クリップをドラッグして開始位置が小節線（2000ms、bpm=120）に吸着する
# ことと、Alt を押しながらでは吸着しないことを、保存された位置で確認する。
# ドラッグは低フレームレートでも取りこぼさないよう、細かいステップと
# 最終位置での静止を入れる（#31 の方式）。
set -u
export DISPLAY=:91
ROOT=/home/hatch/workspace/repos/rust-studio-lane-a
OUT=${1:-/tmp/daw37}
mkdir -p "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
project = {
  "name": "snap drive", "bpm": 120.0,
  "tracks": [{
    "id": 1, "name": "A1", "kind": "audio",
    "clips": [{
      "id": 2, "name": "phrase", "start_ms": 500, "duration_ms": 1000,
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
start_now() { python3 -c "import json;print(json.load(open('$OUT/project.json'))['tracks'][0]['clips'][0]['start_ms'])" 2>/dev/null || echo -1; }
# クリップ本体（開始+500ms の点）を掴んで dx px 右へドラッグする
drag_by() { # dx
  local start grab dx steps i x
  start=$(start_now)
  grab=$(python3 -c "print(round(120 + ($start + 500) * 0.084))")
  dx=$1
  xdotool mousemove "$grab" 48; sleep 0.3
  xdotool mousedown 1; sleep 0.5
  steps=6
  for i in 1 2 3 4 5 6; do
    x=$(python3 -c "print(round($grab + $dx * $i / $steps))")
    xdotool mousemove "$x" 48; sleep 0.25
  done
  sleep 0.5
  xdotool mouseup 1; sleep 0.8
}
shot shot1-before.png
# raw では開始 1952ms になるドラッグ（dx=122px）。小節線 2000ms まで
# 48ms はしきい値 95ms 以内なので吸着するはず
for try in 1 2 3; do
  cur=$(start_now)
  if [ "$cur" = "2000" ]; then break; fi
  drag_by 122
done
[ "$(start_now)" = "2000" ] || { echo "FAIL: snap never happened (start=$(start_now))"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
shot shot2-snapped.png
# Alt を押しながら少し左へ（dx=-4px、raw 開始は約1952ms。吸着すれば
# 2000 に戻るが、無効化されていれば 1952 付近に留まる）
xdotool keydown alt; sleep 0.3
drag_by -4
xdotool keyup alt; sleep 0.5
shot shot3-alt-moved.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys, json, re
out = sys.argv[1]
text = open(f"{out}/gui-stdout.txt").read()
dumps = re.findall(r"after drag: (\{.*?\n\})", text, re.S)
assert dumps, "no after-drag dump"
starts = [json.loads(d)["tracks"][0]["clips"][0]["start_ms"] for d in dumps]
print("clip starts after drags:", starts)
assert 2000 in starts, f"snap to bar line 2000 not observed: {starts}"
final = json.load(open(f"{out}/project.json"))["tracks"][0]["clips"][0]["start_ms"]
print("final start after Alt-drag:", final)
assert final != 2000 and 1930 <= final <= 1975, final
print("SNAP + ALT CHECK OK")
PYEOF
