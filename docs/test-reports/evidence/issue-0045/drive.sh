#!/bin/bash
# Issue #45 証拠ドライブ: 移動・トリムのスナップと Alt での無効化を
# 実アプリのドラッグで検証する。デモ構成: V1（opening 0..2000,
# interview 2500..6500）/ A1 / V2。座標: x = 120 + ms*0.084、V1 行の
# 中心 y=48。再生ヘッドも吸着先なので、吸着が再生ヘッドに吸われた
# 場合は Ctrl+Z で戻して再試行する（最大3回）。
# Xvfb(:92) + lavapipe 前提（実ディスプレイ未検証）。
set -u
cd "$(dirname "$0")/../../../.."
export DISPLAY=:92
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=target/debug/video-editor
OUT=/tmp/issue45
mkdir -p "$OUT"

shot() {
  ffmpeg -y -loglevel error -f x11grab -draw_mouse 0 -video_size 960x720 -i :92 -frames:v 1 "$1"
}
px() {
  python3 -c "from PIL import Image; im=Image.open('$1').convert('RGB'); print(','.join(map(str, im.getpixel(($2,$3)))));"
}
focus() {
  local wid
  wid=$(xdotool search --name "video-editor" | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  sleep 0.3
}
drag() { # x1 y1 x2 y2
  xdotool mousemove "$1" "$2"
  sleep 0.2
  xdotool mousedown 1
  sleep 0.35
  xdotool mousemove "$3" "$4"
  sleep 0.5
  xdotool mouseup 1
  sleep 0.5
}
opening_state() { # 最後の after drag ダンプから opening の (start, duration)
  python3 - <<'EOF'
import json, re
log = open("/tmp/issue45/stdout.log").read()
dumps = re.findall(r"after drag: (\{.*?\n\})", log, re.S)
if not dumps:
    print("none")
else:
    proj = json.loads(dumps[-1])
    c = next(c for t in proj["tracks"] for c in t["clips"] if c["name"] == "opening")
    print(c["start_ms"], c["duration_ms"])
EOF
}

"$BIN" > "$OUT/stdout.log" 2>&1 &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$OUT/stdout.log" && break; sleep 0.5; done
for i in $(seq 1 60); do
  shot "$OUT/wait.png"
  [ "$(px "$OUT/wait.png" 10 12)" != "0,0,0" ] && break
  sleep 0.5
done
shot "$OUT/01-initial.png"
echo "launched"
focus

# 1) 移動スナップ: opening の本体を x=204（1000ms）で掴み x=242 へ。
#    候補は開始 452 / 終端 2452 で、終端が interview の開始 2500 に
#    吸着して開始は 500 になるはず（無吸着なら 452）。
for try in 1 2 3; do
  drag 204 48 242 48
  state=$(opening_state)
  echo "move snap try$try: opening=$state"
  [ "$state" = "500 2000" ] && { echo "OK move snapped to neighbor (start=500)"; break; }
  focus; xdotool key ctrl+z; sleep 2
done
shot "$OUT/02-move-snapped.png"

# 2) Alt+ドラッグ（スナップ無効）: opening（500..2500）の本体を
#    x=246（1500ms）で掴み x=208（1048ms）へ。候補の開始は 48 で、
#    吸着が効けば 0 かグリッドに寄るが、Alt 中は 48 のままのはず。
focus
xdotool keydown alt
drag 246 48 208 48
xdotool keyup alt
state=$(opening_state)
echo "alt drag: opening=$state"
[ "$state" = "48 2000" ] && echo "OK alt disabled snap (start=48 raw)" || echo "FAIL alt drag: $state"
shot "$OUT/03-alt-nosnap.png"

# 3) トリムスナップ: opening（48..2048）の右端 x=292 を掴み x=326 へ。
#    終端候補 2452 が interview の開始 2500 に吸着し、duration は
#    2452 になるはず（無吸着なら 2404）。
for try in 1 2 3; do
  drag 292 48 326 48
  state=$(opening_state)
  echo "trim snap try$try: opening=$state"
  [ "$state" = "48 2452" ] && { echo "OK trim snapped to neighbor (end=2500)"; break; }
  focus; xdotool key ctrl+z; sleep 2
done
shot "$OUT/04-trim-snapped.png"

kill $APP 2>/dev/null
wait $APP 2>/dev/null
echo "DRIVE45_DONE"
