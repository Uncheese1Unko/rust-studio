#!/bin/bash
# Issue #77 証拠ドライブ: ピアノロール見出し帯で、長いタイトルが
# 帯幅に切り捨てられて1行に収まり（ラベル／ヒントに重ならず、
# 帯の下にもはみ出さず）、ヒント文も1行に収まることを確認する。
# Xvfb(:97) + lavapipe 前提（実ディスプレイ未検証）。
set -u
ROOT=/home/hatch/workspace/repos/rust-studio-lane-b
OUT=/tmp/issue77
mkdir -p "$OUT"
export DISPLAY=:97
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=$ROOT/target/debug/daw
cd "$ROOT"

python3 - <<'PYEOF'
import wave, struct, math, json
src = '/tmp/issue77/' + 'a' * 36 + '.wav'
w = wave.open(src, 'wb')
w.setnchannels(1); w.setsampwidth(2); w.setframerate(44100)
w.writeframes(b''.join(struct.pack('<h', int(8000*math.sin(2*math.pi*220*i/44100))) for i in range(44100)))
w.close()
project = {
    "name": "issue77",
    "tracks": [{"id": 1, "name": "A1", "kind": "audio",
                "clips": [{"id": 2, "name": "C" * 50, "start_ms": 0, "duration_ms": 4000,
                           "source": src, "gain": 1.0,
                           "notes": [{"pitch": 69, "start_ms": 0, "duration_ms": 500, "velocity": 100}]}],
                "volume": 1.0, "pan": 0.0,
                "instrument": {"type": "sampler", "source": src, "root_note": 60},
                "tone_id": 10}],
    "tones": [{"id": 10, "name": "T" * 40,
               "instrument": {"type": "sampler", "source": src, "root_note": 60}}],
    "next_id": 20,
}
open('/tmp/issue77/project.json', 'w').write(json.dumps(project))
PYEOF

Xvfb :97 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1

shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :97 -frames:v 1 "$1"; }
wait_drawn() {
  for i in $(seq 1 40); do
    shot "$OUT/probe.png"
    python3 -c "
from PIL import Image
import sys
im = Image.open('$OUT/probe.png').convert('RGB')
sys.exit(0 if im.getpixel((480, 270)) != (0, 0, 0) else 1)
" && break
    sleep 0.5
  done
}
LOG="$OUT/stdout.log"
STUDIO_PROJECT_FILE="$OUT/project.json" "$BIN" > "$LOG" 2> "$OUT/stderr.log" &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$LOG" && break; sleep 0.5; done
wait_drawn

# クリップをダブルクリックしてピアノロールを開く。
# フレームポーリング都合で1回で開かないことがあるため、
# #69 の駆動と同じくログを見ながらリトライする。
focus_win() {
  local wid
  wid=$(xdotool search --name "^daw$" 2>/dev/null | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  sleep 0.3
}
double_click_at() {
  xdotool mousemove "$1" "$2"; sleep 0.2
  xdotool mousedown 1; sleep 0.25; xdotool mouseup 1; sleep 0.05
  xdotool mousedown 1; sleep 0.25; xdotool mouseup 1; sleep 0.4
}
focus_win
for try in 1 2 3 4 5 6 7 8; do
  grep -q "piano roll opened" "$LOG" && break
  double_click_at 288 48
  sleep 0.4
done
if grep -q "piano roll opened" "$LOG"; then echo "OK: piano roll opened"; else echo "FAIL: piano roll never opened"; fi
sleep 0.5
shot "$OUT/01-piano-roll-long-title.png"

python3 - "$OUT/01-piano-roll-long-title.png" <<'PYEOF'
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert('RGB')
PANEL = (255, 253, 249)

def nonbg(x, y):
    r, g, b = im.getpixel((x, y))
    return abs(r - PANEL[0]) + abs(g - PANEL[1]) + abs(b - PANEL[2]) > 60

def dark(x, y, ref):
    r, g, b = im.getpixel((x, y))
    return abs(r - ref[0]) < 45 and abs(g - ref[1]) < 45 and abs(b - ref[2]) < 45

TEXT = (33, 24, 18)
TEXT_SUB = (168, 152, 128)
# 再生ヘッドの縦線（グリッド全高に伸びる暗色列）を割り出し、
# はみ出し検査から除外する（タイトルのはみ出しではない）。
line_cols = set()
for x in range(960):
    n = sum(1 for y in range(60, 300, 8) if nonbg(x, y))
    if n >= 25:
        line_cols.add(x)
def near_line(x):
    return any(abs(x - c) <= 2 for c in line_cols)

# タイトル文字が帯の外（ラベル側 x<108、ヒント側 x>602）に
# 出ていないこと。
leak = [(x, y) for y in range(28, 47)
        for x in list(range(102, 108)) + list(range(602, 608)) if dark(x, y, TEXT)]
print(f"title leak pixels outside band: {len(leak)} (expect 0)")
# タイトル自体が帯の中に1行で描かれていること
inside = sum(1 for y in range(28, 47) for x in range(112, 598) if nonbg(x, y))
print(f"title non-bg pixels inside band: {inside} (expect > 300)")
# ヒントが帯の下にはみ出していないこと（2行折り返しの検出）
spill = sum(1 for y in range(52, 66) for x in range(610, 930) if dark(x, y, TEXT_SUB))
print(f"hint spill pixels below band: {spill} (expect 0)")
# タイトルが帯の下（グリッド側）にはみ出していないこと
tspill = sum(1 for y in range(52, 66) for x in range(112, 598)
             if dark(x, y, TEXT) and not near_line(x))
print(f"title spill pixels below band: {tspill} (expect 0)")
ok = len(leak) == 0 and inside > 300 and spill == 0 and tspill == 0
print("OK: header layout" if ok else "FAIL: header layout")
PYEOF

kill $APP 2>/dev/null; wait $APP 2>/dev/null
kill $XVFB_PID 2>/dev/null
echo "drive done"
