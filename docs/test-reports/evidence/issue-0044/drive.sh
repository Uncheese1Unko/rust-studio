#!/bin/bash
# Issue #44 証拠ドライブ: Ctrl+D 複製 / Ctrl+C コピー / Ctrl+V 貼り付けを
# 実アプリで検証する。デモ構成: V1（opening 0..2000, interview
# 2500..6500）/ A1（bgm 0..8000）/ V2（bg 0..8000）。選択トラックの
# 初期値は V1。再生ヘッドは実時間で進み、末尾で巻き戻る。
# Xvfb(:92) + lavapipe 前提（実ディスプレイ未検証）。
set -u
cd "$(dirname "$0")/../../../.."
export DISPLAY=:92
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=target/debug/video-editor
OUT=/tmp/issue44
mkdir -p "$OUT"

shot() {
  ffmpeg -y -loglevel error -f x11grab -draw_mouse 0 -video_size 960x720 -i :92 -frames:v 1 "$1"
}
px() { # file x y -> "R,G,B"
  python3 -c "from PIL import Image; im=Image.open('$1').convert('RGB'); print(','.join(map(str, im.getpixel(($2,$3)))));"
}
focus() {
  local wid
  wid=$(xdotool search --name "video-editor" | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  sleep 0.3
}
key_until() { # key marker tries
  local key="$1" marker="$2" tries="$3" i
  for i in $(seq 1 "$tries"); do
    focus
    xdotool key "$key"
    sleep 0.8
    grep -q "$marker" "$OUT/stdout.log" && return 0
    sleep 0.7
  done
  return 1
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

# 1) Ctrl+D: 選択中トラック（V1）の再生位置クリップを複製
if key_until ctrl+d "after duplicate:" 20; then echo "OK duplicate fired"; else echo "FAIL duplicate never fired"; fi
python3 - <<'EOF'
import json, re
log = open("/tmp/issue44/stdout.log").read()
dumps = re.findall(r"after duplicate: (\{.*?\n\})", log, re.S)
assert dumps, "no duplicate dump"
proj = json.loads(dumps[-1])
v1 = proj["tracks"][0]
assert len(v1["clips"]) == 3, [c["name"] for c in v1["clips"]]
pair = None
for src in v1["clips"]:
    for dup in v1["clips"]:
        if dup["id"] != src["id"] and dup["start_ms"] == src["start_ms"] + src["duration_ms"] \
           and dup["name"] == src["name"] and dup["source"] == src["source"]:
            pair = (src, dup)
assert pair, "no duplicate pair found"
src, dup = pair
for k in ("duration_ms", "gain", "cutout", "pitch_semitones", "color", "fade_in_ms", "fade_out_ms"):
    assert dup[k] == src[k], (k, dup[k], src[k])
print(f"OK duplicate: {src['name']} {src['start_ms']}..{src['start_ms']+src['duration_ms']} -> dup at {dup['start_ms']}, params equal")
EOF
shot "$OUT/02-duplicated.png"
python3 - <<'EOF'
import json, re
from PIL import Image
log = open("/tmp/issue44/stdout.log").read()
dumps = re.findall(r"after duplicate: (\{.*?\n\})", log, re.S)
proj = json.loads(dumps[-1])
v1 = proj["tracks"][0]
pair = None
for src in v1["clips"]:
    for dup in v1["clips"]:
        if dup["id"] != src["id"] and dup["start_ms"] == src["start_ms"] + src["duration_ms"] \
           and dup["name"] == src["name"]:
            pair = (src, dup)
src, dup = pair
im = Image.open("/tmp/issue44/02-duplicated.png").convert("RGB")
def face(clip, y=48):
    x = int(120 + (clip["start_ms"] + 200) * 0.084)
    return im.getpixel((x, y))
a, b = face(src), face(dup)
assert a == b and a != (23, 23, 28), (a, b)
print(f"OK clip faces match: src {a} == dup {b}")
EOF

# 2) プレビュー: 複製クリップの区間でも同じ source のフレームが出る
python3 - <<'EOF'
import json, re, time
log_path = "/tmp/issue44/stdout.log"
dumps = re.findall(r"after duplicate: (\{.*?\n\})", open(log_path).read(), re.S)
proj = json.loads(dumps[-1])
v1 = proj["tracks"][0]
dup = None
for src in v1["clips"]:
    for d in v1["clips"]:
        if d["id"] != src["id"] and d["start_ms"] == src["start_ms"] + src["duration_ms"] \
           and d["name"] == src["name"]:
            dup = d
lo, hi = dup["start_ms"], dup["start_ms"] + dup["duration_ms"]
deadline = time.time() + 25
found = None
while time.time() < deadline:
    for m in re.finditer(r"preview: source=(\S+) playhead=(\d+)", open(log_path).read()):
        if m.group(1) == dup["source"] and lo <= int(m.group(2)) < hi:
            found = (m.group(1), int(m.group(2)))
    if found:
        break
    time.sleep(0.5)
assert found, f"preview never showed dup source in {lo}..{hi}"
print(f"OK preview over duplicate: source={found[0]} playhead={found[1]} in {lo}..{hi}")
EOF

# 3) Ctrl+C → A1 ヘッダーをクリックして選択 → Ctrl+V
if key_until ctrl+c "copied clip:" 20; then echo "OK copy fired"; else echo "FAIL copy never fired"; fi
COPIED=$(grep "copied clip:" "$OUT/stdout.log" | tail -1 | sed 's/copied clip: //')
echo "copied: $COPIED"
xdotool mousemove 60 96 mousedown 1; sleep 0.45; xdotool mouseup 1; sleep 0.4
if key_until ctrl+v "after paste:" 20; then echo "OK paste fired"; else echo "FAIL paste never fired"; fi
python3 - "$COPIED" <<'EOF'
import json, re, sys
copied = sys.argv[1]
log = open("/tmp/issue44/stdout.log").read()
dumps = re.findall(r"after paste: (\{.*?\n\})", log, re.S)
assert dumps, "no paste dump"
proj = json.loads(dumps[-1])
a1 = next(t for t in proj["tracks"] if t["name"] == "A1")
names = [c["name"] for c in a1["clips"]]
assert copied in names and len(a1["clips"]) == 2, names
pasted = next(c for c in a1["clips"] if c["name"] == copied)
print(f"OK paste: '{copied}' on A1 at {pasted['start_ms']}ms, source={pasted['source']}")
EOF
shot "$OUT/03-pasted.png"

kill $APP 2>/dev/null
wait $APP 2>/dev/null
echo "DRIVE44_DONE"
