#!/bin/bash
# Issue #85 証拠ドライブ: スクロールバー（描画・ドラッグ・終端
# クランプ）、表示幅比例ホイール、横ホイール／Shift+ホイール、
# 中ボタンパン、既存操作（Ctrl+ホイールズーム、クリップドラッグ）の
# 回帰を実アプリで確認する。Xvfb(:95) + lavapipe 前提
# （実ディスプレイ未検証）。
# X11 ではホイール1クリックが press/release の2イベントになる
# （#46 の知見）ため、1クリック = 2ノッチとして検証する。
set -u
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=/tmp/issue85
mkdir -p "$OUT"
export DISPLAY=:95
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=$ROOT/target/debug/video-editor
cd "$ROOT"

Xvfb :95 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1

shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :95 -frames:v 1 "$1"; }
focus_win() {
  local wid
  wid=$(xdotool search --name "video-editor" 2>/dev/null | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  sleep 0.3
}
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
last_offset() { grep "^scroll offset:" "$1" | tail -1 | sed 's/scroll offset: //'; }
px() { python3 -c "from PIL import Image; im=Image.open('$1').convert('RGB'); print(','.join(map(str, im.getpixel(($2,$3)))));"; }

# ---- Launch A: 2トラック × 21,000ms。帯は y=120..132 ----
cat > "$OUT/projectA.json" <<'EOF'
{"name": "issue85 A", "tracks": [
 {"id": 1, "name": "V1", "kind": "video", "clips": [{"id": 3, "name": "long-v", "start_ms": 0, "duration_ms": 21000, "source": "demo/opening.mp4", "gain": 1.0}], "volume": 1.0, "pan": 0.0},
 {"id": 2, "name": "A1", "kind": "audio", "clips": [{"id": 4, "name": "long-a", "start_ms": 0, "duration_ms": 21000, "source": "demo/bgm.wav", "gain": 1.0}], "volume": 1.0, "pan": 0.0}
], "next_id": 5}
EOF
STUDIO_PROJECT_FILE="$OUT/projectA.json" "$BIN" > "$OUT/stdoutA.log" 2> "$OUT/stderrA.log" &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$OUT/stdoutA.log" && break; sleep 0.5; done
wait_drawn
focus_win
shot "$OUT/01-initial.png"
# つまみ: offset 0 では x=120 から幅 840*10000/21000=400。帯 y=126。
echo "thumb pixel(200,126): $(px "$OUT/01-initial.png" 200 126) (thumb shade expected)"
echo "band pixel(700,126): $(px "$OUT/01-initial.png" 700 126) (band shade expected)"
python3 - "$OUT/01-initial.png" <<'PYEOF'
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert('RGB')
thumb = im.getpixel((200, 126)); band = im.getpixel((700, 126))
assert 100 < thumb[0] < 135 and thumb[0] > band[0] + 50, (thumb, band)
print(f"OK scrollbar drawn: thumb={thumb} band={band}")
PYEOF

# 1) スクロールバーのドラッグ: つまみ内 (320,126) を掴み (530,126) へ。
#    grab_dx=200 なのでつまみ左端は 330 → offset = 210/840*21000 = 5250。
xdotool mousemove 320 126; sleep 0.2; xdotool mousedown 1; sleep 0.3
xdotool mousemove 425 126; sleep 0.3; xdotool mousemove 530 126; sleep 0.3
xdotool mouseup 1; sleep 0.5
OFF=$(last_offset "$OUT/stdoutA.log"); echo "offset after thumb drag: $OFF (expect 5250)"
[ "$OFF" = "5250" ] && echo "OK thumb drag" || echo "FAIL thumb drag"
grep -q "scrollbar drag end: offset 5250" "$OUT/stdoutA.log" && echo "OK drag end logged"

# 2) つまみを右端まで: 現在のつまみは x=330..730。(400,126) を掴み
#    (940,126) へ → raw は上限超えでクランプされ offset = 11000。
xdotool mousemove 400 126; sleep 0.2; xdotool mousedown 1; sleep 0.3
xdotool mousemove 700 126; sleep 0.3; xdotool mousemove 940 126; sleep 0.3
xdotool mouseup 1; sleep 0.5
OFF=$(last_offset "$OUT/stdoutA.log"); echo "offset after drag to end: $OFF (expect 11000 = max)"
[ "$OFF" = "11000" ] && echo "OK end clamp" || echo "FAIL end clamp"
shot "$OUT/02-dragged-end.png"
python3 - "$OUT/02-dragged-end.png" <<'PYEOF'
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert('RGB')
# 上限ではつまみ右端が画面右端（960）に一致する: x=950 はつまみ色、
# x=200 は帯色になる。
t = im.getpixel((950, 126)); b = im.getpixel((200, 126))
assert t[0] > b[0] + 50, (t, b)
print(f"OK thumb at right end: thumb={t} band={b}")
PYEOF

# 3) 中ボタンパン: (516,48) から (600,48) へ左方向へ内容を引く
#    （dx = 516-600 = -84px = -1000ms）→ offset 11000 → 10000。
xdotool mousemove 516 48; sleep 0.2; xdotool mousedown 2; sleep 0.3
xdotool mousemove 558 48; sleep 0.3; xdotool mousemove 600 48; sleep 0.3
xdotool mouseup 2; sleep 0.5
OFF=$(last_offset "$OUT/stdoutA.log"); echo "offset after middle pan: $OFF (expect 10000)"
[ "$OFF" = "10000" ] && echo "OK middle pan" || echo "FAIL middle pan"

# 4) ホイール: 1クリック = 2ノッチ = 表示幅の 2/10 = 2000ms。
xdotool mousemove 500 60; sleep 0.2
xdotool click 5; sleep 0.5
OFF=$(last_offset "$OUT/stdoutA.log"); echo "offset after wheel down x1: $OFF (expect 11000 clamp)"
[ "$OFF" = "11000" ] && echo "OK wheel proportional + clamp" || echo "FAIL wheel down"
xdotool click 4; sleep 0.5
OFF=$(last_offset "$OUT/stdoutA.log"); echo "offset after wheel up x1: $OFF (expect 9000)"
[ "$OFF" = "9000" ] && echo "OK wheel up" || echo "FAIL wheel up"

# 5) Shift+ホイール下 → 進む（9000 → 11000）。
xdotool keydown shift; sleep 0.2; xdotool click 5; sleep 0.3; xdotool keyup shift; sleep 0.5
OFF=$(last_offset "$OUT/stdoutA.log"); echo "offset after shift+wheel: $OFF (expect 11000)"
[ "$OFF" = "11000" ] && echo "OK shift wheel" || echo "FAIL shift wheel"

# 6) 横ホイール: このスタック（winit/X11）ではボタン6が
#    LineDelta x=+1、ボタン7が x=−1 として届くことを実測した
#    （一時デバッグ出力で確認）。x 成分でも表示幅比例で両方向に
#    スクロールできることを見る。
xdotool click 7; sleep 0.5
OFF=$(last_offset "$OUT/stdoutA.log"); echo "offset after h-wheel (btn7, x-): $OFF (expect 9000)"
[ "$OFF" = "9000" ] && echo "OK h-wheel backward" || echo "FAIL h-wheel backward"
xdotool click 6; sleep 0.5
OFF=$(last_offset "$OUT/stdoutA.log"); echo "offset after h-wheel (btn6, x+): $OFF (expect 11000)"
[ "$OFF" = "11000" ] && echo "OK h-wheel forward" || echo "FAIL h-wheel forward"

# 7) Ctrl+ホイールズーム（既存操作の回帰）。
xdotool keydown ctrl; sleep 0.2; xdotool click 4; sleep 0.3; xdotool click 4; sleep 0.3; xdotool keyup ctrl; sleep 0.5
grep -q "^zoom:" "$OUT/stdoutA.log" && echo "OK ctrl wheel zoom: $(grep '^zoom:' "$OUT/stdoutA.log" | tail -1)" || echo "FAIL ctrl zoom"
kill $APP 2>/dev/null; wait $APP 2>/dev/null

# ---- Launch B: クリップドラッグの回帰（#46 と同じ形） ----
cat > "$OUT/projectB.json" <<'EOF'
{"name": "issue85 B", "tracks": [
 {"id": 1, "name": "V1", "kind": "video", "clips": [{"id": 3, "name": "opening", "start_ms": 1000, "duration_ms": 2000, "source": "demo/opening.mp4", "gain": 1.0}], "volume": 1.0, "pan": 0.0}
], "next_id": 4}
EOF
cp "$OUT/projectB.json" "$OUT/projectB.orig.json"
STUDIO_PROJECT_FILE="$OUT/projectB.json" "$BIN" > "$OUT/stdoutB.log" 2> "$OUT/stderrB.log" &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$OUT/stdoutB.log" && break; sleep 0.5; done
wait_drawn
focus_win
# クリップ中心 (288,48) を +84px（=+1000ms）右へドラッグ。
xdotool mousemove 288 48; sleep 0.2; xdotool mousedown 1; sleep 0.45
xdotool mousemove 330 48; sleep 0.4; xdotool mousemove 372 48; sleep 0.4
xdotool mouseup 1; sleep 0.8
python3 - "$OUT/projectB.json" <<'PYEOF'
import json, sys
p = json.load(open(sys.argv[1]))
start = p['tracks'][0]['clips'][0]['start_ms']
assert start == 2000, start
print(f"OK clip drag regression: start_ms 1000 -> {start}")
PYEOF
kill $APP 2>/dev/null; wait $APP 2>/dev/null

kill $XVFB_PID 2>/dev/null
echo "DRIVE85_DONE"
