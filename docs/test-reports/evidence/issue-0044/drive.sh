#!/bin/bash
# Issue #44 証拠ドライブ（レビュー round 2 対応で再記録）:
# パラメータ調整済みのプロジェクト（STUDIO_PROJECT_FILE）を読み込み、
# Ctrl+D 複製 / Ctrl+C コピー / Ctrl+V 貼り付けを実アプリで検証する。
# V1 の interview（0..8000ms、トラック全体）は gain 0.5 / pitch +3 / cutout on / color mint /
# fade 300・400 に調整済みで、複製・貼り付けでも一致することを
# 状態ダンプで確かめる。キーは「マーカー件数が増えるまで送らない」
# 方式で単発発火を保証する（前回はマーカー遅延で複製が二重発火した）。
# Xvfb(:92) + lavapipe 前提（実ディスプレイ未検証）。
set -u
cd "$(dirname "$0")/../../../.."
export DISPLAY=:92
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=target/debug/video-editor
OUT=/tmp/issue44
mkdir -p "$OUT"

# 調整済みプロジェクトを用意する（デモ構成と同じ形で interview を調整）
python3 - <<'EOF'
import json
proj = {
  "name": "issue44 tuned",
  "tracks": [
    {"id": 1, "name": "V1", "kind": "video", "volume": 1.0, "pan": 0.0, "color": None, "clips": [
      {"id": 5, "name": "interview", "start_ms": 0, "duration_ms": 8000, "source": "demo/interview.mp4",
       "offset_ms": 0, "gain": 0.5, "cutout": True, "pitch_semitones": 3.0, "color": "mint",
       "fade_in_ms": 300, "fade_out_ms": 400}
    ]},
    {"id": 2, "name": "A1", "kind": "audio", "volume": 1.0, "pan": 0.0, "color": None, "clips": [
      {"id": 6, "name": "bgm", "start_ms": 0, "duration_ms": 8000, "source": "demo/bgm.wav",
       "offset_ms": 0, "gain": 1.0, "cutout": False, "pitch_semitones": 0.0, "color": None,
       "fade_in_ms": 0, "fade_out_ms": 0}
    ]},
    {"id": 3, "name": "V2", "kind": "video", "volume": 1.0, "pan": 0.0, "color": None, "clips": [
      {"id": 7, "name": "bg", "start_ms": 0, "duration_ms": 8000, "source": "demo/bg.mp4",
       "offset_ms": 0, "gain": 1.0, "cutout": False, "pitch_semitones": 0.0, "color": None,
       "fade_in_ms": 0, "fade_out_ms": 0}
    ]}
  ],
  "next_id": 8
}
open("/tmp/issue44/project.json", "w").write(json.dumps(proj, indent=2))
print("tuned project written")
EOF

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
count() { grep -c "$1" "$OUT/stdout.log" 2>/dev/null || true; }
key_until() { # key marker tries: マーカー件数が増えたら成功（二重発火防止）
  local key="$1" marker="$2" tries="$3" i before now waited
  before=$(count "$marker")
  for i in $(seq 1 "$tries"); do
    focus
    xdotool key "$key"
    for waited in 1 2 3 4 5 6; do
      sleep 0.5
      now=$(count "$marker")
      [ "$now" -gt "$before" ] && return 0
    done
  done
  return 1
}

STUDIO_PROJECT_FILE=/tmp/issue44/project.json "$BIN" > "$OUT/stdout.log" 2>&1 &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$OUT/stdout.log" && break; sleep 0.5; done
grep -q "project file mode:" "$OUT/stdout.log" && echo "OK project file mode"
for i in $(seq 1 60); do
  shot "$OUT/wait.png"
  [ "$(px "$OUT/wait.png" 10 12)" != "0,0,0" ] && break
  sleep 0.5
done
shot "$OUT/01-initial.png"
echo "launched"

# 1) Ctrl+D: interview は V1 全体を覆うので、再生ヘッドがどこでも複製できる。
if key_until ctrl+d "after duplicate:" 30; then echo "OK duplicate fired"; else echo "FAIL duplicate never fired"; fi
python3 - <<'EOF'
import json, re
log = open("/tmp/issue44/stdout.log").read()
dumps = re.findall(r"after duplicate: (\{.*?\n\})", log, re.S)
assert len(dumps) == 1, f"duplicate fired {len(dumps)} times (expected exactly 1)"
proj = json.loads(dumps[-1])
v1 = proj["tracks"][0]
assert len(v1["clips"]) == 2, [c["name"] for c in v1["clips"]]
src = next(c for c in v1["clips"] if c["name"] == "interview" and c["start_ms"] == 0)
dup = next(c for c in v1["clips"] if c["id"] != src["id"] and c["start_ms"] == src["start_ms"] + src["duration_ms"])
for k in ("name", "duration_ms", "source", "gain", "cutout", "pitch_semitones", "color", "fade_in_ms", "fade_out_ms"):
    assert dup[k] == src[k], (k, dup[k], src[k])
assert src["gain"] == 0.5 and src["cutout"] is True and src["color"] == "mint", src
print(f"OK duplicate exactly once: interview -> dup at {dup['start_ms']}, tuned params equal (gain {dup['gain']}, cutout {dup['cutout']}, color {dup['color']}, fade {dup['fade_in_ms']}/{dup['fade_out_ms']}, pitch {dup['pitch_semitones']})")
EOF
shot "$OUT/02-duplicated.png"
python3 - <<'EOF'
import json, re
from PIL import Image
log = open("/tmp/issue44/stdout.log").read()
dumps = re.findall(r"after duplicate: (\{.*?\n\})", log, re.S)
proj = json.loads(dumps[-1])
v1 = proj["tracks"][0]
src = next(c for c in v1["clips"] if c["name"] == "interview" and c["start_ms"] == 0)
dup = next(c for c in v1["clips"] if c["id"] != src["id"] and c["start_ms"] == src["start_ms"] + src["duration_ms"])
im = Image.open("/tmp/issue44/02-duplicated.png").convert("RGB")
def face(clip, y=48):
    x = int(120 + (clip["start_ms"] + 200) * 0.084)
    return im.getpixel((x, y))
a, b = face(src), face(dup)
# 色 mint を付けたクリップ同士なので面色が一致する（種別色ではない）
assert a == b, (a, b)
print(f"OK clip faces match (user color mint): src {a} == dup {b}")
EOF

# 2) プレビュー: 複製クリップの区間でも同じ source のフレームが出る
python3 - <<'EOF'
import json, re, time
log_path = "/tmp/issue44/stdout.log"
dumps = re.findall(r"after duplicate: (\{.*?\n\})", open(log_path).read(), re.S)
proj = json.loads(dumps[-1])
v1 = proj["tracks"][0]
src = next(c for c in v1["clips"] if c["name"] == "interview" and c["start_ms"] == 0)
dup = next(c for c in v1["clips"] if c["id"] != src["id"] and c["start_ms"] == src["start_ms"] + src["duration_ms"])
lo, hi = dup["start_ms"], dup["start_ms"] + dup["duration_ms"]
deadline = time.time() + 30
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
if key_until ctrl+c "copied clip:" 30; then echo "OK copy fired"; else echo "FAIL copy never fired"; fi
COPIED=$(grep "copied clip:" "$OUT/stdout.log" | tail -1 | sed 's/copied clip: //')
echo "copied: $COPIED"
xdotool mousemove 60 96 mousedown 1; sleep 0.45; xdotool mouseup 1; sleep 0.4
if key_until ctrl+v "after paste:" 30; then echo "OK paste fired"; else echo "FAIL paste never fired"; fi
python3 - "$COPIED" <<'EOF'
import json, re, sys
copied = sys.argv[1]
log = open("/tmp/issue44/stdout.log").read()
dumps = re.findall(r"after paste: (\{.*?\n\})", log, re.S)
assert len(dumps) == 1, f"paste fired {len(dumps)} times"
proj = json.loads(dumps[-1])
a1 = next(t for t in proj["tracks"] if t["name"] == "A1")
assert len(a1["clips"]) == 2, [c["name"] for c in a1["clips"]]
pasted = next(c for c in a1["clips"] if c["name"] == copied)
if copied == "interview":
    assert pasted["gain"] == 0.5 and pasted["cutout"] is True and pasted["color"] == "mint" \
        and pasted["fade_in_ms"] == 300 and pasted["fade_out_ms"] == 400, pasted
    print(f"OK paste: 'interview' on A1 at {pasted['start_ms']}ms with tuned params preserved")
else:
    print(f"OK paste: '{copied}' on A1 at {pasted['start_ms']}ms, source={pasted['source']}")
EOF
shot "$OUT/03-pasted.png"

kill $APP 2>/dev/null
wait $APP 2>/dev/null
echo "DRIVE44_DONE"
