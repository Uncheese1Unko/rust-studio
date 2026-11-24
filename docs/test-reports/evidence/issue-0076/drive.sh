#!/bin/bash
# Issue #76 証拠ドライブ: 音色パネルの折り返し（20音色で2行化、
# 右端を超えない）、チップの右クリック削除（未割り当て音色）、
# ダブルクリックでの改名を実アプリで確認する。
# Xvfb(:97) + lavapipe 前提（実ディスプレイ未検証）。
# 知見: xdotool の click は短すぎてフレームポーリングに拾われない
# ことがあるため、押下を 0.4 秒保持する。
set -u
ROOT=/home/hatch/workspace/repos/rust-studio-lane-b
OUT=/tmp/issue76
mkdir -p "$OUT"
export DISPLAY=:97
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=$ROOT/target/debug/daw
cd "$ROOT"

python3 - <<'PYEOF'
import wave, struct, math, json
w = wave.open('/tmp/issue76/tone.wav', 'wb')
w.setnchannels(1); w.setsampwidth(2); w.setframerate(44100)
w.writeframes(b''.join(struct.pack('<h', int(8000*math.sin(2*math.pi*220*i/44100))) for i in range(44100)))
w.close()
tones = [
    {"id": 10 + i, "name": f"Tone {i+1:02d}",
     "instrument": {"type": "sampler", "source": "/tmp/issue76/tone.wav", "root_note": 60}}
    for i in range(20)
]
project = {
    "name": "issue76",
    "tracks": [{"id": 1, "name": "A1", "kind": "audio",
                "clips": [{"id": 2, "name": "long", "start_ms": 0, "duration_ms": 21000,
                           "source": "/tmp/issue76/tone.wav", "gain": 1.0}],
                "volume": 1.0, "pan": 0.0}],
    "tones": tones,
    "next_id": 40,
}
open('/tmp/issue76/project.json', 'w').write(json.dumps(project))
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
wait_log() {
  for i in $(seq 1 $(( $2 * 4 ))); do
    grep -q "$1" "$3" && return 0
    sleep 0.25
  done
  return 1
}
# PANEL_SUNK (243,231,217) がチップ面。背景は (251,244,236)。
is_chip() { # $1=file $2=x $3=y
  python3 - "$1" "$2" "$3" <<'PYEOF'
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert('RGB')
r, g, b = im.getpixel((int(sys.argv[2]), int(sys.argv[3])))
print("chip" if abs(r-243) < 12 and abs(g-231) < 12 and abs(b-217) < 12 else f"other({r},{g},{b})")
PYEOF
}
last_dump() { # 最後の after tone 系ログ以降の pretty JSON を切り出す
  python3 - "$LOG" <<'PYEOF2'
import sys
text = open(sys.argv[1]).read()
idx = max(text.rfind('\nafter tone: {'), text.rfind('\nafter tone rename: {'))
if idx < 0:
    print('{}')
    sys.exit()
start = text.index('{', idx)
depth = 0
for i in range(start, len(text)):
    if text[i] == '{':
        depth += 1
    elif text[i] == '}':
        depth -= 1
        if depth == 0:
            print(text[start:i + 1])
            sys.exit()
print('{}')
PYEOF2
}
tones_count() { last_dump | python3 -c "import sys, json; print(len(json.load(sys.stdin)['tones']))"; }
LOG="$OUT/stdout.log"
STUDIO_PROJECT_FILE="$OUT/project.json" "$BIN" > "$LOG" 2> "$OUT/stderr.log" &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$LOG" && break; sleep 0.5; done
wait_drawn
sleep 0.5
shot "$OUT/01-wrapped.png"
# 1行目（y=358）と2行目（y=384）の左端にチップ面がある（折り返し）
echo "row1 x=90 y=358 (expect chip): $(is_chip "$OUT/01-wrapped.png" 90 358)"
echo "row2 x=90 y=384 (expect chip): $(is_chip "$OUT/01-wrapped.png" 90 384)"
# 右端（x=950）はチップではない（パネル外にはみ出していない）
echo "row1 x=950 y=358 (expect other): $(is_chip "$OUT/01-wrapped.png" 950 358)"

# 右クリックで未割り当ての音色（2行目の先頭チップ = Tone 13）を削除
xdotool mousemove 105 391; sleep 0.2; xdotool mousedown 3; sleep 0.4; xdotool mouseup 3; sleep 0.5
if wait_log "^after tone:" 5 "$LOG"; then echo "OK: right-click delete logged"; else echo "FAIL: right-click delete"; fi
echo "tones after right-click delete: $(tones_count) (expect 19)"

# ダブルクリックで改名セッションに入り、Z を足して Enter で確定する
xdotool mousemove 105 365; sleep 0.2
xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.2
xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.5
if wait_log "^tone rename begin: tone 10" 5 "$LOG"; then echo "OK: rename begin"; else echo "FAIL: rename begin"; fi
xdotool type "Z"; sleep 0.4
xdotool key Return; sleep 0.8
if grep -q "^after tone rename:" "$LOG"; then echo "OK: rename committed"; else echo "FAIL: rename commit"; fi
last_dump | python3 -c "
import sys, json
tones = json.load(sys.stdin)['tones']
name = [t['name'] for t in tones if t['id'] == 10][0]
print(f'tone 10 name after rename: {name} (expect Tone 01Z)')
print('OK: renamed' if name == 'Tone 01Z' else 'FAIL: renamed')
"
shot "$OUT/02-after-ops.png"

# 通常の左クリック（割り当てトグル）が従来どおり動く
N_BEFORE=$(grep -c "^after tone:" "$LOG")
xdotool mousemove 200 365; sleep 0.2; xdotool mousedown 1; sleep 0.4; xdotool mouseup 1; sleep 0.8
N_AFTER=$(grep -c "^after tone:" "$LOG")
echo "after tone lines: $N_BEFORE -> $N_AFTER (expect +1)"
[ "$N_AFTER" = "$((N_BEFORE + 1))" ] && echo "OK: chip click toggle" || echo "FAIL: chip click toggle"

kill $APP 2>/dev/null; wait $APP 2>/dev/null
kill $XVFB_PID 2>/dev/null
echo "drive done"
