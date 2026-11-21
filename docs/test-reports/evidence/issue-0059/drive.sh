#!/bin/bash
# Issue #59 証拠ドライブ: 色パネルでトラック色（Coral）とクリップ色
# （Lavender）を設定し、ダンプ・ヘッダー帯・クリップ面のピクセル・
# 色クリアを検証する。デモ構成: V1（opening 0..2000ms）/ A1 / V2。
# パネル（3トラック時）: 上端 y=228、TRACK 行中心 257、CLIP 行 283。
# Xvfb + lavapipe 前提（実ディスプレイ未検証）。
set -u
cd "$(dirname "$0")/../../../.."
export DISPLAY=:99
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=target/debug/video-editor
OUT=/tmp/issue59
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
px() { # file x y -> "R,G,B"
  python3 -c "from PIL import Image; im=Image.open('$1').convert('RGB'); print(','.join(map(str, im.getpixel(($2,$3)))));"
}

"$BIN" > "$OUT/stdout.log" 2>&1 &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$OUT/stdout.log" && break; sleep 0.5; done
# 描画開始待ち（起動直後は黒い期間がある、#57 の知見）
for i in $(seq 1 60); do
  shot "$OUT/wait.png"
  [ "$(px "$OUT/wait.png" 10 12)" != "0,0,0" ] && break
  sleep 0.5
done
shot "$OUT/01-initial.png"
# 未設定のトラック帯は種別色（映像 = (64,115,217)）
[ "$(px "$OUT/01-initial.png" 3 48)" = "64,115,217" ] \
  && echo "OK initial bar is kind color" || echo "FAIL initial bar: $(px "$OUT/01-initial.png" 3 48)"

# 1) CLIP 行 Lavender → 再生位置のクリップに lavender。
# 再生位置が V1 のクリップ外（隙間・末尾）のことがあるため、
# ダンプに現れるまでクリックを繰り返す（V1 のクリップは 8000ms 中
# 6000ms を占める）。
for i in $(seq 1 12); do
  click 290 283
  grep -A 60 "after color" "$OUT/stdout.log" | grep -q '"color": "lavender"' && break
  sleep 1
done
grep -A 60 "after color" "$OUT/stdout.log" | grep -q '"color": "lavender"' \
  && echo "OK clip color lavender in dump" || echo "FAIL clip color lavender"
shot "$OUT/02-clip-lavender.png"
python3 - <<'EOF'
import json, re
from PIL import Image
log = open("/tmp/issue59/stdout.log").read()
dumps = re.findall(r"after color: (\{.*?\n\})", log, re.S)
proj = json.loads(dumps[-1])
clip = next(c for t in proj["tracks"] for c in t["clips"] if c.get("color") == "lavender")
x = int(120 + clip["start_ms"] * 0.084) + 30
im = Image.open("/tmp/issue59/02-clip-lavender.png").convert("RGB")
got = im.getpixel((x, 36))
assert got == (166, 140, 242), (clip["name"], x, got)
print(f"OK clip face lavender ({clip['name']} at x={x})")
EOF

# 2) TRACK 行 Coral → 選択中トラック（V1）に coral。帯で確認
click 134 257
grep -A 60 "after color" "$OUT/stdout.log" | grep -q '"color": "coral"' \
  && echo "OK track color coral in dump" || echo "FAIL track color coral"
shot "$OUT/03-track-coral.png"
[ "$(px "$OUT/03-track-coral.png" 3 48)" = "242,127,89" ] \
  && echo "OK header bar coral" || echo "FAIL header bar: $(px "$OUT/03-track-coral.png" 3 48)"

# 3) TRACK 行「なし」→ クリア（最後のダンプで V1 の色が null、帯も種別色へ）
click 82 257
python3 - <<'EOF'
import json, re
log = open("/tmp/issue59/stdout.log").read()
dumps = re.findall(r"after color: (\{.*?\n\})", log, re.S)
last = json.loads(dumps[-1])
assert last["tracks"][0]["color"] is None, last["tracks"][0]
print("OK track color cleared")
EOF
shot "$OUT/04-cleared.png"
[ "$(px "$OUT/04-cleared.png" 3 48)" = "64,115,217" ] \
  && echo "OK header bar back to kind color" || echo "FAIL header bar after clear: $(px "$OUT/04-cleared.png" 3 48)"

kill $APP 2>/dev/null
wait $APP 2>/dev/null
echo "DRIVE59_DONE"
