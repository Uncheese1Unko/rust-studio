#!/bin/bash
# Issue #73 acceptance drive (GUI):
# 1) 音色パネルで「+ TONE FROM TRACK」を押すと、選択中トラックの
#    楽器から音色が作られ、プロジェクトファイルに保存される。
# 2) 音色チップを押すと選択中トラックへ割り当てられ、チップが
#    アクセント色で灯り、ファイルの tone_id に反映される。
# 3) 「DELETE ASSIGNED」で音色が削除され、割り当ても外れる。
# 座標は tones.rs のレイアウト（1トラック時）から決める:
# パネル上端 336、行中心 y=365。作成ボタン x 72..204、
# 音色チップ（tone220.wav、幅 95）x 72..167、
# 削除ボタン x 311..443。
set -u
export DISPLAY=:97
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=$ROOT/docs/test-reports/evidence/issue-0073
LIVE=/tmp/issue0073-project.json

# 素材（220Hz サイン、1秒）を作る。
python3 - <<'PYEOF'
import math, struct, wave
w = wave.open('/tmp/issue0073-tone220.wav', 'wb')
w.setnchannels(1)
w.setsampwidth(2)
w.setframerate(44100)
frames = b''.join(
    struct.pack('<h', int(0.5 * 32767 * math.sin(2 * math.pi * 220 * i / 44100)))
    for i in range(44100)
)
w.writeframes(frames)
w.close()
PYEOF
sed -e "s|__TONEWAV__|/tmp/issue0073-tone220.wav|" \
  "$OUT/project.seed.json" > "$LIVE"

Xvfb :97 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE="$LIVE" "$ROOT/target/debug/daw" \
  > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
cleanup() { kill $GUI_PID $XVFB_PID 2>/dev/null; }
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 1
shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :97 -frames:v 1 "$OUT/$1"; }
click() { # x y （エッジ検出のため少し長めに押す）
  xdotool mousemove "$1" "$2" mousedown 1
  sleep 0.35
  xdotool mousemove "$1" "$2" mouseup 1
  sleep 1
}
assert_live() { # python 式（project を読み True/False）
  python3 - "$LIVE" "$1" <<'PYEOF'
import json, sys
with open(sys.argv[1]) as f:
    project = json.load(f)
sys.exit(0 if eval(sys.argv[2]) else 1)
PYEOF
}

shot shot1-no-tones.png
# --- 1) 作成 ---
click 138 365
assert_live "len(project['tones']) == 1 and project['tones'][0]['name'] == 'issue0073-tone220.wav'" \
  || { echo "FAIL: tone not created/saved"; cleanup; exit 1; }
shot shot2-tone-created.png
# --- 2) 割り当て ---
click 119 365
assert_live "project['tracks'][0]['tone_id'] == project['tones'][0]['id']" \
  || { echo "FAIL: tone not assigned"; cleanup; exit 1; }
shot shot3-tone-assigned.png
python3 - "$OUT/shot3-tone-assigned.png" <<'PYEOF' || { echo "FAIL: assigned chip not lit"; cleanup; exit 1; }
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert('RGB')
# チップ矩形（x 72..167, y 355..375）の内側でアクセント色の
# 画素数を数える（中央は名前の文字と重なるため面で判定する）。
hits = 0
for y in range(356, 375):
    for x in range(73, 167):
        r, g, b = im.getpixel((x, y))
        if r > 200 and g < 110 and 100 < b < 190:
            hits += 1
print(f"accent pixels in assigned chip: {hits}")
sys.exit(0 if hits > 200 else 1)
PYEOF
# --- 3) 削除 ---
click 377 365
assert_live "len(project['tones']) == 0 and project['tracks'][0]['tone_id'] is None" \
  || { echo "FAIL: tone not deleted/unassigned"; cleanup; exit 1; }
shot shot4-tone-deleted.png
kill $GUI_PID 2>/dev/null
kill $XVFB_PID 2>/dev/null
echo "DRIVE OK"
