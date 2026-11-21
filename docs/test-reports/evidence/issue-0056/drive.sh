#!/bin/bash
# Issue #56 acceptance drive: DAW GUI under Xvfb + lavapipe.
# トラック追加（+ TRACK クリック）→ リネーム（ヘッダーのダブルクリック
# → 打鍵 → Enter）→ 削除（ヘッダー右端の ×）を実アプリで通す。
# クリックはアプリのポーリングが拾えるよう down/up を分けて送る
# （#14 の実績方式。ダブルクリックは hold 0.15 / gap 0.1）。
set -u
export DISPLAY=:99
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=${1:-/tmp/daw56}
mkdir -p "$OUT"
Xvfb :99 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
"$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 1
shot() { ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/$1"; }
click() { # x y
  xdotool mousemove "$1" "$2"; sleep 0.2
  xdotool mousedown 1; sleep 0.4; xdotool mouseup 1; sleep 0.6
}
shot shot1-initial.png
# 追加: 最終行直下のヘッダー列（x=60, y=96）をクリック → A2 が増える
click 60 96
shot shot2-added.png
# リネーム: 1行目のヘッダーをダブルクリック → 編集開始。
# 既存名 A1 を消して LEAD と打ち、Enter で確定する
xdotool mousemove 60 48; sleep 0.2
xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.1
xdotool mousedown 1; sleep 0.15; xdotool mouseup 1
sleep 0.8
xdotool key BackSpace; sleep 0.2; xdotool key BackSpace; sleep 0.2; xdotool key BackSpace; sleep 0.2
xdotool type "LEAD"
sleep 0.6
shot shot3-renaming.png
xdotool key Return
sleep 1
shot shot4-renamed.png
# 削除: 2行目（A2）のヘッダー右端の ×（x=111, y=96）をクリック
click 111 96
shot shot5-removed.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys
from PIL import Image
out = sys.argv[1]
BACKGROUND = (251, 244, 236)  # #FBF4EC
PANEL = (255, 253, 249)       # #FFFDF9
def px(path, x, y):
    return Image.open(path).convert("RGB").getpixel((x, y))
# 2行目の帯（x=400, y=96）: 初期は背景（追加行）、追加後はトラック行、
# 削除後は背景に戻る
print("shot1 row2 band:", px(f"{out}/shot1-initial.png", 400, 96), "expect", BACKGROUND)
print("shot2 row2 band:", px(f"{out}/shot2-added.png", 400, 96), "expect", PANEL)
print("shot5 row2 band:", px(f"{out}/shot5-removed.png", 400, 96), "expect", BACKGROUND)
PYEOF
grep -c "after track add" "$OUT/gui-stdout.txt" | sed 's/^/after track add count: /'
grep -o "rename begin.*" "$OUT/gui-stdout.txt"
grep -c "after rename" "$OUT/gui-stdout.txt" | sed 's/^/after rename count: /'
grep -o '"name": "LEAD"' "$OUT/gui-stdout.txt" | head -1
grep -c "after track remove" "$OUT/gui-stdout.txt" | sed 's/^/after track remove count: /'
