#!/bin/bash
# Issue #60 証拠ドライブ: 削除→追加でトラック名が重複しないこと。
# A2 を追加 → A1 を削除 → 追加すると、修正後は「A3」になる
# （旧方式では「A2」が重複した）。
# Xvfb + lavapipe 前提（実ディスプレイ未検証）。
set -u
cd "$(dirname "$0")/../../../.."
export DISPLAY=:99
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=target/debug/daw
OUT=/tmp/issue60
mkdir -p "$OUT"

click() { # x y（分割クリック: アプリはフレームポーリングのため）
  xdotool mousemove "$1" "$2" mousedown 1
  sleep 0.45
  xdotool mouseup 1
  sleep 0.4
}
shot() {
  ffmpeg -y -loglevel error -f x11grab -draw_mouse 0 -video_size 960x720 -i :99 -frames:v 1 "$1"
}
px() {
  python3 -c "from PIL import Image; im=Image.open('$1').convert('RGB'); print(','.join(map(str, im.getpixel(($2,$3)))));"
}

"$BIN" > "$OUT/stdout.log" 2>&1 &
APP=$!
for i in $(seq 1 40); do grep -q "^elements:" "$OUT/stdout.log" && break; sleep 0.5; done
for i in $(seq 1 40); do
  shot "$OUT/wait.png"
  [ "$(px "$OUT/wait.png" 10 12)" != "0,0,0" ] && break
  sleep 0.5
done
shot "$OUT/01-initial.png"

# 1) 追加 → A2
click 60 96
grep -q '"name": "A2"' "$OUT/stdout.log" && echo "OK added A2" || echo "FAIL add A2"

# 2) A1（1行目）の × をクリック → A1 削除
click 111 48
grep -q "after track remove" "$OUT/stdout.log" && echo "OK removed A1" || echo "FAIL remove A1"

# 3) 追加 → A3 になるはず（A2 と重複しない）
click 60 96
python3 - <<'EOF'
import json, re
log = open("/tmp/issue60/stdout.log").read()
dumps = re.findall(r"after track add: (\{.*?\n\})", log, re.S)
names = [t["name"] for t in json.loads(dumps[-1])["tracks"]]
assert names == ["A2", "A3"], names
print("OK names after delete-add:", names)
EOF
shot "$OUT/02-after-delete-add.png"

kill $APP 2>/dev/null
wait $APP 2>/dev/null
echo "DRIVE60_DONE"
