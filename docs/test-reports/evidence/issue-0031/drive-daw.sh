#!/bin/bash
# Issue #31 証拠ドライブ（DAW）: クリップをドラッグで移動 → Ctrl+Z で
# 戻る → Ctrl+Shift+Z でやり直す → Ctrl+Z → Ctrl+Y でもやり直せる。
# 状態はプロジェクトファイル（GUI が保存し直す）の start_ms で判定する。
# Xvfb + lavapipe 前提（実ディスプレイ未検証）。
set -u
export DISPLAY=:99
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=${1:-/tmp/issue31-daw}
mkdir -p "$OUT"
ffmpeg -y -loglevel error -f lavfi -i sine=frequency=220:duration=10 "$OUT/tone.wav"
python3 "$ROOT/docs/test-reports/evidence/issue-0031/make_project_daw.py" "$OUT/tone.wav" "$OUT/project.json"

Xvfb :99 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE="$OUT/project.json" "$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 60); do grep -q "^elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/$1"; }
for i in $(seq 1 60); do
  shot probe.png
  python3 -c "
from PIL import Image
import sys
img = Image.open('$OUT/probe.png').convert('RGB')
sys.exit(0 if img.getpixel((10, 12)) != (0, 0, 0) else 1)
" && break
  sleep 1
done
sleep 1
start_ms() { python3 -c "import json; print(json.load(open('$OUT/project.json'))['tracks'][0]['clips'][0]['start_ms'])"; }

key_hold() { # ctrl+z 等をポーリングが拾えるよう長押しで送る
  local IFS='+'
  local parts=($1)
  local p
  for p in "${parts[@]}"; do xdotool keydown "$p"; sleep 0.15; done
  sleep 0.7
  for (( idx=${#parts[@]}-1; idx>=0; idx-- )); do xdotool keyup "${parts[$idx]}"; sleep 0.15; done
  sleep 0.5
}
drag_clip() { # クリップ中心 (288,48) を +84px（=+1000ms）右へ
  xdotool mousemove 288 48; sleep 0.2
  xdotool mousedown 1; sleep 0.45
  xdotool mousemove 330 48; sleep 0.4
  xdotool mousemove 372 48; sleep 0.4
  xdotool mouseup 1; sleep 0.8
}
shot 01-initial.png
echo "initial start_ms: $(start_ms) (expect 1000)"
drag_clip
shot 02-after-drag.png
echo "after drag start_ms: $(start_ms) (expect 2000)"
key_hold ctrl+z
shot 03-after-undo.png
echo "after ctrl+z start_ms: $(start_ms) (expect 1000)"
key_hold ctrl+shift+z
shot 04-after-redo.png
echo "after ctrl+shift+z start_ms: $(start_ms) (expect 2000)"
key_hold ctrl+z
echo "after ctrl+z(2) start_ms: $(start_ms) (expect 1000)"
key_hold ctrl+y
shot 05-after-ctrl-y.png
echo "after ctrl+y start_ms: $(start_ms) (expect 2000)"
# 原位置へ戻すドラッグ（#31 レビュー round 2 対応）: 一度動かして
# 同じ位置へ戻したジェスチャは履歴に積まれない。直後の Ctrl+Z は
# 幽霊エントリではなく本物の直前エントリを消費し、1000 に戻る。
drag_return() { # クリップ中心 (372,48) を +84px 動かして同じ位置へ戻す
  xdotool mousemove 372 48; sleep 0.2
  xdotool mousedown 1; sleep 0.45
  xdotool mousemove 456 48; sleep 0.4
  xdotool mousemove 372 48; sleep 0.4
  xdotool mouseup 1; sleep 0.8
}
drag_return
shot 06-after-return-drag.png
echo "after return-drag start_ms: $(start_ms) (expect 2000)"
key_hold ctrl+z
echo "after ctrl+z after return-drag start_ms: $(start_ms) (expect 1000)"
key_hold ctrl+shift+z
echo "after redo after return-drag start_ms: $(start_ms) (expect 2000)"
# Shift 先行離し（#31 レビュー対応）: Ctrl+Shift+Z の後、Z より先に
# Shift を離しても undo が誤発しないこと。redo は空なので押下自体も無動作。
UNDO_BEFORE=$(grep -c "after undo:" "$OUT/gui-stdout.txt")
xdotool keydown ctrl; sleep 0.3; xdotool keydown shift; sleep 0.3; xdotool keydown z; sleep 0.9
xdotool keyup shift; sleep 0.9; xdotool keyup z; sleep 0.3; xdotool keyup ctrl; sleep 0.6
UNDO_AFTER=$(grep -c "after undo:" "$OUT/gui-stdout.txt")
echo "undo lines after shift-first release: before=$UNDO_BEFORE after=$UNDO_AFTER (expect equal)"
echo "start_ms after shift-first release: $(start_ms) (expect 2000)"
# 編集中ガード（DAW）: リネーム開始 → Ctrl+Z → 履歴は動かず、編集も継続する
dblclick() { # x y
  xdotool mousemove "$1" "$2"; sleep 0.2
  xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.1
  xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.6
}
dblclick 60 48
grep -o "rename begin.*" "$OUT/gui-stdout.txt" | head -1
UNDO_BEFORE=$(grep -c "after undo:" "$OUT/gui-stdout.txt")
key_hold ctrl+z
UNDO_AFTER=$(grep -c "after undo:" "$OUT/gui-stdout.txt")
echo "undo lines during rename: before=$UNDO_BEFORE after=$UNDO_AFTER (expect equal)"
echo "start_ms during rename: $(start_ms) (expect 2000)"
shot 07-renaming-guard.png
xdotool key Escape; sleep 0.6
grep -o "rename cancelled" "$OUT/gui-stdout.txt" | head -1
grep -c "after undo:" "$OUT/gui-stdout.txt" | sed 's/^/after undo count: /'
grep -c "after redo:" "$OUT/gui-stdout.txt" | sed 's/^/after redo count: /'
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
