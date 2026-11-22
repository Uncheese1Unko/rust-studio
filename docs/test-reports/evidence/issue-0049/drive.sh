#!/bin/bash
# Issue #49 証拠ドライブ: X キーで境界にクロスディゾルブ（500ms）を
# 付け外しできること、プレビューで境界区間の映像が2クリップの
# ブレンドになることを確認する。素材は赤系（A）と青系（B）の
# フレーム番号入り動画。Xvfb(:92) + lavapipe 前提（実ディスプレイ
# 未検証）。
set -u
export DISPLAY=:92
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=~/workspace/repos/rust-studio-lane-b/target/debug/video-editor
OUT=/tmp/issue49
SRC=/tmp/trans49
mkdir -p "$OUT" "$SRC/a" "$SRC/b"

[ -S /tmp/.X11-unix/X92 ] || { setsid nohup Xvfb :92 -screen 0 960x720x24 > /tmp/xvfb92.log 2>&1 < /dev/null & sleep 2; }

shot() {
  ffmpeg -y -loglevel error -f x11grab -draw_mouse 0 -video_size 960x720 -i :92 -frames:v 1 "$1"
}
preview_mean() {
  python3 - "$1" <<'EOF'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB").crop((600, 200, 840, 340))
px = list(im.getdata())
n = len(px)
print(f"{sum(p[0] for p in px)/n:.0f},{sum(p[1] for p in px)/n:.0f},{sum(p[2] for p in px)/n:.0f}")
EOF
}
hold_x() {
  xdotool mousemove 480 300; sleep 0.2
  xdotool keydown x; sleep 1.2; xdotool keyup x
}

python3 - <<'EOF'
from PIL import Image, ImageDraw
for name, color in [("a", (200, 40, 40)), ("b", (40, 40, 200))]:
    for i in range(30):
        img = Image.new("RGB", (320, 180), color)
        d = ImageDraw.Draw(img)
        d.text((20, 20), f"{name.upper()} FRAME {i}", fill=(255, 255, 255))
        img.save(f"/tmp/trans49/{name}/f{i:03}.png")
print("frames generated")
EOF
ffmpeg -y -loglevel error -framerate 10 -i "$SRC/a/f%03d.png" -c:v libx264 -pix_fmt yuv420p "$SRC/sceneA.mp4"
ffmpeg -y -loglevel error -framerate 10 -i "$SRC/b/f%03d.png" -c:v libx264 -pix_fmt yuv420p "$SRC/sceneB.mp4"

make_project() {
  python3 - "$1" <<'EOF'
import json, sys
trans = int(sys.argv[1])
def clip(cid, name, start, dur, source, tms):
    return {"id": cid, "name": name, "start_ms": start, "duration_ms": dur,
            "source": source, "offset_ms": 0, "gain": 1.0,
            "pitch_semitones": 0.0, "cutout": False, "color": None,
            "fade_in_ms": 0, "fade_out_ms": 0, "title_text": "",
            "transition_in_kind": "cross_dissolve" if tms > 0 else "none",
            "transition_in_ms": tms}
proj = {"name": "issue49", "tracks": [
    {"id": 1, "name": "V1", "kind": "video", "volume": 1.0, "pan": 0.0, "color": None,
     "clips": [clip(3, "sceneA", 0, 3000, "/tmp/trans49/sceneA.mp4", 0),
               clip(4, "sceneB", 3000, 3000, "/tmp/trans49/sceneB.mp4", trans)]},
    {"id": 2, "name": "A1", "kind": "audio", "volume": 1.0, "pan": 0.0, "color": None, "clips": []}],
    "next_id": 5}
open("/tmp/issue49/project.json", "w").write(json.dumps(proj, indent=2))
print("project written")
EOF
}

# ---- フェーズA: GUI の X キーで付け外し ----
make_project 0
rm -rf /tmp/rust-studio-video-editor-media
STUDIO_PROJECT_FILE=/tmp/issue49/project.json "$BIN" > "$OUT/stdout-a.log" 2>&1 &
APP=$!
for i in $(seq 1 60); do grep -q "preview textures loaded" "$OUT/stdout-a.log" && break; sleep 0.5; done
# 再生ヘッドが sceneB の中（3300ms 以降）に入るまで待つ
for i in $(seq 1 40); do
  PH=$(grep -oE "playhead=[0-9]+" "$OUT/stdout-a.log" | tail -1 | cut -d= -f2)
  [ -n "$PH" ] && [ "$PH" -ge 3100 ] && break
  sleep 0.25
done
hold_x
for i in $(seq 1 40); do grep -q "after transition:" "$OUT/stdout-a.log" && break; sleep 0.25; done
python3 - <<'EOF'
import json
proj = json.load(open("/tmp/issue49/project.json"))
b = [c for c in proj["tracks"][0]["clips"] if c["name"] == "sceneB"][0]
assert b["transition_in_kind"] == "cross_dissolve" and b["transition_in_ms"] == 500, b
print("OK X key set cross_dissolve 500ms on sceneB")
EOF
shot "$OUT/01-transition-set.png"
# もう一度 X で解除（再生ヘッドが sceneB のうちに続けて押す）
hold_x
for i in $(seq 1 40); do [ "$(grep -c "after transition:" "$OUT/stdout-a.log")" -ge 2 ] && break; sleep 0.25; done
python3 - <<'EOF'
import json
proj = json.load(open("/tmp/issue49/project.json"))
b = [c for c in proj["tracks"][0]["clips"] if c["name"] == "sceneB"][0]
assert b["transition_in_kind"] == "none" and b["transition_in_ms"] == 0, b
print("OK X key cleared the transition")
EOF
kill $APP 2>/dev/null; wait $APP 2>/dev/null

# ---- フェーズB: プレビューのブレンド（窓 2000ms で観測しやすく） ----
make_project 2000
STUDIO_PROJECT_FILE=/tmp/issue49/project.json "$BIN" > "$OUT/stdout-b.log" 2>&1 &
APP=$!
for i in $(seq 1 60); do grep -q "preview textures loaded" "$OUT/stdout-b.log" && break; sleep 0.5; done
# 窓の前（A のみ）: playhead 2600..2950
for i in $(seq 1 60); do
  PH=$(grep -oE "playhead=[0-9]+" "$OUT/stdout-b.log" | tail -1 | cut -d= -f2)
  [ -n "$PH" ] && [ "$PH" -ge 2600 ] && break
  sleep 0.2
done
shot "$OUT/02-before-window.png"
echo "before window preview mean (expect red): $(preview_mean "$OUT/02-before-window.png")"
# 窓の中ほど（alpha 0.50 のログが出たら即撮影）
for i in $(seq 1 80); do grep -q "transition blend: alpha=0.50" "$OUT/stdout-b.log" && break; sleep 0.15; done
grep "transition blend:" "$OUT/stdout-b.log" | tail -2
shot "$OUT/03-mid-blend.png"
echo "mid blend preview mean (expect purple-ish): $(preview_mean "$OUT/03-mid-blend.png")"
# 窓の後（B のみ）: playhead 5100..
for i in $(seq 1 80); do
  PH=$(grep -oE "playhead=[0-9]+" "$OUT/stdout-b.log" | tail -1 | cut -d= -f2)
  [ -n "$PH" ] && [ "$PH" -ge 5100 ] && break
  sleep 0.2
done
shot "$OUT/04-after-window.png"
echo "after window preview mean (expect blue): $(preview_mean "$OUT/04-after-window.png")"
python3 - "$OUT/02-before-window.png" "$OUT/03-mid-blend.png" "$OUT/04-after-window.png" <<'EOF'
import sys
from PIL import Image
def mean(p):
    im = Image.open(p).convert("RGB").crop((600, 200, 840, 340))
    px = list(im.getdata()); n = len(px)
    return tuple(sum(c[i] for c in px) / n for i in range(3))
a, m, b = mean(sys.argv[1]), mean(sys.argv[2]), mean(sys.argv[3])
print(f"A={a} mid={m} B={b}")
assert a[0] > 120 and a[2] < 110, a          # A は赤系
assert b[2] > 120 and b[0] < 110, b          # B は青系
assert 70 < m[0] < 170 and 70 < m[2] < 170, m  # 中間は両方混ざる
print("OK preview blends inside the window, plain outside")
EOF
# ブレンド済みフレームのキャッシュ画像そのものも数値で確認
python3 - <<'EOF'
import glob
from PIL import Image
files = glob.glob("/tmp/rust-studio-video-editor-media/blend/*.png")
assert files, "no blend cache files"
def mean(p):
    im = Image.open(p).convert("RGB").resize((32, 18))
    px = list(im.getdata()); n = len(px)
    return tuple(sum(c[i] for c in px) / n for i in range(3))
means = [(f, mean(f)) for f in files]
mid = [(f, m) for f, m in means if 60 < m[0] < 180 and 60 < m[2] < 180]
print(f"blend cache files: {len(files)}, mid-blend files: {len(mid)}")
assert mid, means[:3]
print("OK blend cache contains a true mid-blend frame")
EOF
kill $APP 2>/dev/null; wait $APP 2>/dev/null
echo "DRIVE49_DONE"
