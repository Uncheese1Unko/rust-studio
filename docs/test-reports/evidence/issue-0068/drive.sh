#!/bin/bash
# Issue #68 acceptance drive: DAW GUI under Xvfb + lavapipe (:94).
# MCP で作ったノートプロジェクト（サンプラー楽器＋ノート3つ）を
# 実アプリで開き、起動ダンプに楽器とノートが現れることと、
# タイムラインにノートクリップが描かれることを確認する。
# 音そのものの証拠は daw --export-wav の実バイナリ書き出し解析
# （melody-analysis.txt）と単体テストが担う。
set -u
export DISPLAY=:94
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=$ROOT/docs/test-reports/evidence/issue-0068
Xvfb :94 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE="$OUT/project.json" "$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :94 -frames:v 1 "$OUT/$1"; }
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
# フォーカスを当ててからスクリーンショット（描画を進めるため）
wid=$(xdotool search --name "^daw$" 2>/dev/null | head -1)
[ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
xdotool mousemove 60 48; sleep 0.2; xdotool mousedown 1; sleep 0.4; xdotool mouseup 1; sleep 1
shot shot1-timeline.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
text = open(f"{out}/gui-stdout.txt").read()
marker = "daw (issue #14: timeline UI)\n"
idx = text.find(marker)
assert idx >= 0, "no initial dump"
dump, _ = json.JSONDecoder().raw_decode(text[idx + len(marker):])
track = dump["tracks"][0]
assert track["instrument"] == {"type": "sampler", "source": "/tmp/tone220.wav", "root_note": 60}, track["instrument"]
notes = track["clips"][0]["notes"]
assert [n["pitch"] for n in notes] == [60, 64, 72], notes
print("initial dump OK: sampler instrument + notes [60, 64, 72] visible in the app")
from PIL import Image
im = Image.open(f"{out}/shot1-timeline.png").convert("RGB")
w, h = im.size
# タイムライン帯（上半分）に背景クリーム以外の彩色ピクセルがあること
saturated = 0
for y in range(60, 260, 4):
    for x in range(0, w, 4):
        r, g, b = im.getpixel((x, y))
        if max(r, g, b) - min(r, g, b) > 40:
            saturated += 1
print(f"timeline saturated sample pixels: {saturated}")
assert saturated > 50, "note clip not visible on the timeline"
print("DRIVE OK")
PYEOF
