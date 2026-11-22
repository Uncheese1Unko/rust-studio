#!/bin/bash
# Issue #74 drive: ピアノロールの見出し帯で、パネルラベル
# 「06 PIANO ROLL」とタイトル（クリップ名＋楽器）が重ならない
# ことを確認する。修正前はタイトルの中心が 120px 固定で左へ
# 広がり、ラベル域（x<110）にタイトルの濃色画素が 46 個あった
# （issue-0070 の証拠で実測）。
set -u
export DISPLAY=:98
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=$ROOT/docs/test-reports/evidence/issue-0074
LIVE=/tmp/issue0074-project.json
cp "$OUT/project.seed.json" "$LIVE"
Xvfb :98 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE="$LIVE" "$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
cleanup() { kill $GUI_PID $XVFB_PID 2>/dev/null; }
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :98 -frames:v 1 "$OUT/$1"; }
for i in $(seq 1 40); do
  shot probe.png
  if python3 -c "
from PIL import Image
import sys
im = Image.open('$OUT/probe.png').convert('RGB')
sys.exit(0 if im.getpixel((480, 270)) != (0, 0, 0) else 1)
"; then break; fi
  sleep 0.5
done
focus_win() {
  local wid
  wid=$(xdotool search --name "^daw$" 2>/dev/null | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  xdotool mousemove 60 48; sleep 0.2; xdotool mousedown 1; sleep 0.3; xdotool mouseup 1; sleep 0.4
}
double_click_at() {
  xdotool mousemove "$1" "$2"; sleep 0.2
  xdotool mousedown 1; sleep 0.25; xdotool mouseup 1; sleep 0.05
  xdotool mousedown 1; sleep 0.25; xdotool mouseup 1; sleep 0.4
}
focus_win
for try in 1 2 3 4 5; do
  grep -q "piano roll opened: clip 2" "$OUT/gui-stdout.txt" && break
  focus_win
  double_click_at 300 48
  sleep 0.4
done
grep -q "piano roll opened: clip 2" "$OUT/gui-stdout.txt" || { echo "FAIL: roll never opened"; cleanup; exit 1; }
sleep 0.8
shot shot1-roll-header.png
python3 - "$OUT/shot1-roll-header.png" <<'PYEOF' || { echo "FAIL: header still overlaps"; cleanup; exit 1; }
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert('RGB')
def ink(x0, x1):
    n = 0
    for y in range(24, 50):
        for x in range(x0, x1):
            r, g, b = im.getpixel((x, y))
            if r < 120 and g < 110 and b < 100:
                n += 1
    return n
label_zone = ink(0, 110)
title_zone = ink(150, 560)
print(f"ink pixels: label zone {label_zone}, title zone {title_zone}")
# ラベルはグレージュ（濃色ではない）ので、ラベル域の濃色は
# タイトルの侵入を意味する。タイトル自体は帯の中にあること。
sys.exit(0 if label_zone == 0 and title_zone > 10 else 1)
PYEOF
cleanup
echo "DRIVE OK"
