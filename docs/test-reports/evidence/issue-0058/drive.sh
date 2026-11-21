#!/bin/bash
# Issue #58 証拠ドライブ: 色パネルでトラック色（Mint）とクリップ色
# （Sky）を設定し、ダンプ・帯ピクセル・再生中 ACCENT 上書き・
# 未設定クリップ色の描画・色クリアを検証する。
# Xvfb + lavapipe 前提（実ディスプレイ未検証）。
set -u
cd "$(dirname "$0")/../../../.."
export DISPLAY=:99
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=target/debug/daw
OUT=/tmp/issue58
mkdir -p "$OUT"
python3 docs/test-reports/evidence/issue-0058/make_project.py

click() { # x y（分割クリック: アプリはフレームポーリングのため）
  xdotool mousemove "$1" "$2" mousedown 1
  sleep 0.45
  xdotool mouseup 1
  sleep 0.4
}
shot() {
  ffmpeg -y -loglevel error -f x11grab -draw_mouse 0 -video_size 960x720 -i :99 -frames:v 1 "$1"
}
px() { # file x y -> "R,G,B"
  python3 -c "from PIL import Image; im=Image.open('$1').convert('RGB'); print(','.join(map(str, im.getpixel(($2,$3)))));"
}

STUDIO_PROJECT_FILE=/tmp/issue58-project.json "$BIN" > "$OUT/stdout.log" 2>&1 &
APP=$!
# 起動待ち（elements 行＋画面が描画されるまでポーリング）
for i in $(seq 1 40); do grep -q "^elements:" "$OUT/stdout.log" && break; sleep 0.5; done
for i in $(seq 1 40); do
  shot "$OUT/wait.png"
  [ "$(px "$OUT/wait.png" 10 12)" != "0,0,0" ] && break
  sleep 0.5
done
shot "$OUT/01-initial.png"

# 1) CLIP 行 Sky（再生位置は main の中のはず）→ main に sky
click 264 307
grep -A 40 "after color" "$OUT/stdout.log" | grep -q '"color": "sky"' \
  && echo "OK clip color sky in dump" || echo "FAIL clip color sky"
# 再生中は色が付いていても ACCENT で描かれる（ADR-0013）
shot "$OUT/02-playing-accent.png"
[ "$(px "$OUT/02-playing-accent.png" 300 32)" = "255,46,136" ] \
  && echo "OK playing clip renders ACCENT" || echo "FAIL playing clip pixel: $(px "$OUT/02-playing-accent.png" 300 32)"

# 2) TRACK 行 Mint → トラック色。ヘッダー帯とミキサー帯で確認
click 212 281
grep -A 40 "after color" "$OUT/stdout.log" | grep -q '"color": "mint"' \
  && echo "OK track color mint in dump" || echo "FAIL track color mint"
shot "$OUT/03-track-mint.png"
[ "$(px "$OUT/03-track-mint.png" 3 48)" = "196,237,230" ] \
  && echo "OK header bar mint" || echo "FAIL header bar: $(px "$OUT/03-track-mint.png" 3 48)"
[ "$(px "$OUT/03-track-mint.png" 3 138)" = "196,237,230" ] \
  && echo "OK mixer bar mint" || echo "FAIL mixer bar: $(px "$OUT/03-track-mint.png" 3 138)"

# 3) 再生位置が main を抜けたら（5 秒以降）main は Sky 面で描かれる
for i in $(seq 1 30); do
  shot "$OUT/poll.png"
  [ "$(px "$OUT/poll.png" 300 32)" = "191,220,232" ] && break
  sleep 1
done
cp "$OUT/poll.png" "$OUT/04-clip-sky.png"
[ "$(px "$OUT/04-clip-sky.png" 300 32)" = "191,220,232" ] \
  && echo "OK clip face sky after playhead left" || echo "FAIL clip face sky: $(px "$OUT/04-clip-sky.png" 300 32)"

# 4) TRACK 行「なし」→ 色クリア（最後のダンプでトラックの色が null）
click 82 281
python3 - <<'EOF'
import json, re
log = open("/tmp/issue58/stdout.log").read()
dumps = re.findall(r"after color: (\{.*?\n\})", log, re.S)
last = json.loads(dumps[-1])
assert last["tracks"][0]["color"] is None, last["tracks"][0]
print("OK track color cleared")
EOF
shot "$OUT/05-cleared.png"

kill $APP 2>/dev/null
wait $APP 2>/dev/null
echo "DRIVE58_DONE"
