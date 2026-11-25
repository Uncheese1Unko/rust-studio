#!/usr/bin/env bash
# Issue #94 の駆動証拠: GUI（Xvfb :97 + lavapipe、実ディスプレイ
# 未検証）でエフェクトパネルを実際にクリック／ドラッグし、アプリが
# 出力する状態ダンプでチェーンの変化を確認する。
# 使い方: bash drive.sh [REPO_ROOT]
# 前提: xdotool / ffmpeg / python3（PIL）。座標は 960x540 の
# 既定ウィンドウでの実測（チップ行 y=422、fxpanel のレイアウト計算
# とスクリーンショットのピクセル実測で確定）。
set -u
ROOT="${1:-$(cd "$(dirname "$0")/../../../.." && pwd)}"
HERE="$(cd "$(dirname "$0")" && pwd)"
DAW="$ROOT/target/debug/daw"
export DISPLAY=:97
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
W=/tmp/issue0094
mkdir -p "$W"
exec > >(tee "$HERE/drive-log.txt") 2>&1

python3 - "$W/project.json" << 'PYEOF'
import json, sys
p = {"version": 1, "name": "fx94", "sample_rate": 44100, "next_id": 10,
     "tones": [],
     "tracks": [{"id": 1, "name": "A1", "kind": "audio", "volume": 0.8,
                 "pan": 0.0, "instrument": None, "clips": [],
                 "tone_id": None, "effects": []}],
     "master_volume": 0.8, "playhead_ms": 0, "loop_region": None,
     "markers": [], "tempo_bpm": 120.0}
json.dump(p, open(sys.argv[1], "w"))
print("seed project written (empty chain)")
PYEOF

Xvfb :97 -screen 0 1280x900x24 > "$W/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 2
STUDIO_PROJECT_FILE="$W/project.json" "$DAW" \
  > "$HERE/gui-stdout.txt" 2> "$HERE/gui-stderr.txt" &
APP_PID=$!
cleanup() { kill $APP_PID 2>/dev/null; kill $XVFB_PID 2>/dev/null; }
for i in $(seq 1 60); do
  grep -q "elements:" "$HERE/gui-stdout.txt" 2>/dev/null && break; sleep 0.5
done
grep -q "project file mode: $W/project.json" "$HERE/gui-stdout.txt" \
  || { echo "FAIL: project file not loaded"; cleanup; exit 1; }
WID=$(xdotool search --name "^daw$" | head -1)
[ -n "$WID" ] && xdotool windowfocus "$WID" 2>/dev/null
sleep 1
snap() {
  ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 1280x900 \
    -i :97 -frames:v 1 "$1"
}
click() {
  xdotool mousemove "$1" "$2"; sleep 0.2
  xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.5
}
assert_effects() {
  python3 - "$HERE/gui-stdout.txt" "$1" "$2" "$3" << 'PYEOF' || { echo "FAIL: $1"; cleanup; exit 1; }
import re, sys
text = open(sys.argv[1]).read()
dumps = list(re.finditer(r'"effects": \[(.*?)\]', text, re.S))
body = dumps[-1].group(1)
types = re.findall(r'"type": "(\w+)"', body)
bypassed = re.findall(r'"bypassed": (\w+)', body)
decay = re.findall(r'"decay": ([\d.]+)', body)
want_types = sys.argv[3].split(",") if sys.argv[3] else []
assert types == want_types, f"types {types} != {want_types}"
if sys.argv[4]:
    m = re.fullmatch(r"(\w+)(?:\[(\d+)\])?=(.+)", sys.argv[4])
    key, idx, want = m.group(1), int(m.group(2) or 0), m.group(3)
    got = {"bypassed": bypassed, "decay": decay}[key]
    assert len(got) > idx and got[idx] == want, f"{key}[{idx}] {got} != {want}"
print(f"OK: effects={types} bypassed={bypassed} decay={decay}")
PYEOF
}

snap "$HERE/shot1-empty.png"
click 98 422    # +REV
assert_effects "add reverb" "reverb" ""
click 254 422   # +DLY（スロット1個分の右）
assert_effects "add delay" "reverb,delay" ""
click 118 422   # スロット0（REVERB）を選択
grep -q "fx selected: slot 0" "$HERE/gui-stdout.txt" \
  || { echo "FAIL: slot select"; cleanup; exit 1; }
echo "OK: slot 0 selected"
# DECAY スライダーを 0.5 → 0.75 へドラッグ
xdotool mousemove 350 447; sleep 0.2; xdotool mousedown 1; sleep 0.2
xdotool mousemove 390 447; sleep 0.2; xdotool mousemove 430 447; sleep 0.2
xdotool mouseup 1; sleep 0.6
assert_effects "decay drag" "reverb,delay" "decay=0.75"
click 459 422   # DEL（2スロット時の位置）
assert_effects "delete selected" "delay" ""
click 118 422   # DELAY を選択
click 309 422   # +EQ（スロット1個分の右）
assert_effects "add eq" "delay,eq3" ""
click 501 422   # ↑（2スロット時の位置）
assert_effects "move up" "eq3,delay" ""
click 533 422   # ↓（2スロット時の位置）
assert_effects "move down" "delay,eq3" ""
click 575 422   # BYP（2スロット時の位置、選択中は index 1 の EQ）
assert_effects "bypass" "delay,eq3" "bypassed[1]=true"
snap "$HERE/shot2-final.png"
python3 - "$HERE/shot2-final.png" << 'PYEOF' || { echo "FAIL: pixels"; cleanup; exit 1; }
# 選択中チップと BYP ボタンがアクセント色（#FF2E88）で灯ること
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert("RGB")
accent = (255, 46, 136)
n = sum(1 for y in range(405, 440) for x in range(0, 700)
        if im.getpixel((x, y)) == accent)
print(f"accent pixels in fx chip row: {n}")
assert n > 200, "selected chip / BYP not lit"
print("pixels OK")
PYEOF
# トラック切替（Issue #94 レビュー N-3）: トラックを追加すると
# 新トラックが選択され、パネルはそのトラック（空チェーン）を
# 対象にする。+EQ がトラック2に入り、トラック1は不変であること。
click 60 96     # +TRACK
click 211 507   # +EQ（2トラック時のパネル位置、空チェーン配置）
python3 - "$HERE/gui-stdout.txt" << 'PYEOF' || { echo "FAIL: track switch"; cleanup; exit 1; }
import json, sys
text = open(sys.argv[1]).read()
start = text.rfind("after fx: ")
assert start >= 0
obj, _ = json.JSONDecoder().raw_decode(text[start + len("after fx: "):])
chains = [[s["effect"]["type"] for s in t["effects"]] for t in obj["tracks"]]
print("chains per track:", chains)
assert chains[0] == ["delay", "eq3"], chains
assert chains[1] == ["eq3"], chains
print("OK: fx panel followed the selected track")
PYEOF
snap "$HERE/shot3-track2.png"
cleanup
echo "DRIVE OK"
