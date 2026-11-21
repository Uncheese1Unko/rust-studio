#!/bin/bash
# Issue #28 evidence drive: DAW under Xvfb + lavapipe.
# 修正前（HEAD のバイナリ）と修正後のバイナリで同じ初期画面を撮り、
# ピッチ表示（画面左下）のピクセル色を比較する。
# usage: drive.sh <before-binary> <after-binary> <outdir>
set -u
export DISPLAY=:99
ROOT=/home/hatch/workspace/repos/rust-studio
BEFORE=${1:?before binary}
AFTER=${2:?after binary}
OUT=${3:-/tmp/daw28}
mkdir -p "$OUT"
Xvfb :99 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
shoot() {
  local bin=$1 name=$2
  (cd "$ROOT" && "$bin" > "$OUT/$name-stdout.txt" 2> "$OUT/$name-stderr.txt") &
  local pid=$!
  for i in $(seq 1 40); do grep -q "elements:" "$OUT/$name-stdout.txt" 2>/dev/null && break; sleep 0.5; done
  sleep 0.8
  ffmpeg -y -loglevel error -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/$name.png"
  kill "$pid" 2>/dev/null
  wait "$pid" 2>/dev/null
}
shoot "$BEFORE" shot-before
shoot "$AFTER" shot-after
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys
from PIL import Image
out = sys.argv[1]
GOLD = (242, 191, 89)       # 旧リテラル rgb(0.95, 0.75, 0.35)
TEXT_SUB = (168, 152, 128)  # theme::TEXT_SUB #A89880
BASE = (251, 244, 236)      # 背景 #FBF4EC
# ピッチ表示の領域: nannou 座標 (-win.w/2+130, -win.h/2+16) を中心とする
# 12px の文字。画面座標では x=130, y=524 付近。余裕を持って切り出す。
BOX = (30, 500, 300, 539)
def count_in(path, color):
    im = Image.open(path).convert("RGB").crop(BOX)
    return list(im.getdata()).count(color)
def px(path, x, y):
    return Image.open(path).convert("RGB").getpixel((x, y))
for name in ["shot-before.png", "shot-after.png"]:
    p = f"{out}/{name}"
    print(name,
          "bg(480,300)==#FBF4EC:", px(p, 480, 300) == BASE,
          "gold in pitch-label region:", count_in(p, GOLD),
          "text_sub in pitch-label region:", count_in(p, TEXT_SUB))
PYEOF
