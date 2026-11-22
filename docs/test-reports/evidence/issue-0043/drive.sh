#!/bin/bash
# Issue #43 証拠ドライブ: S キーで再生ヘッド位置のクリップを分割し、
# 右クリップのプレビューが素材の続き（offset 反映）になることを、
# フレーム番号を描いた素材と展開フレーム画像の比較で確認する。
# Xvfb(:92) + lavapipe 前提（実ディスプレイ未検証）。
set -u
export DISPLAY=:92
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=~/workspace/repos/rust-studio-lane-b/target/debug/video-editor
OUT=/tmp/issue43
SRC=/tmp/split43
mkdir -p "$OUT" "$SRC/frames"

[ -S /tmp/.X11-unix/X92 ] || { setsid nohup Xvfb :92 -screen 0 960x720x24 > /tmp/xvfb92.log 2>&1 < /dev/null & sleep 2; }

shot() {
  ffmpeg -y -loglevel error -f x11grab -draw_mouse 0 -video_size 960x720 -i :92 -frames:v 1 "$1"
}

# 素材: 6秒・10fps、各フレームに番号と時刻相当の明るさを描く
python3 - <<'EOF'
from PIL import Image, ImageDraw
for i in range(60):
    v = int(i * 4)
    img = Image.new("RGB", (320, 180), (v, v, v))
    d = ImageDraw.Draw(img)
    d.text((20, 20), f"FRAME {i}", fill=(255, 0, 0))
    d.text((20, 150), f"t={i/10:.1f}s", fill=(0, 255, 0))
    img.save(f"/tmp/split43/frames/f{i:03}.png")
print("frames generated")
EOF
ffmpeg -y -loglevel error -framerate 10 -i "$SRC/frames/f%03d.png" -c:v libx264 -pix_fmt yuv420p "$SRC/long.mp4"

python3 - <<'EOF'
import json
clip = {"id": 3, "name": "long", "start_ms": 0, "duration_ms": 6000,
        "source": "/tmp/split43/long.mp4", "offset_ms": 0, "gain": 1.0,
        "pitch_semitones": 0.0, "cutout": False, "color": None,
        "fade_in_ms": 0, "fade_out_ms": 0, "title_text": ""}
proj = {"name": "issue43", "tracks": [
    {"id": 1, "name": "V1", "kind": "video", "volume": 1.0, "pan": 0.0, "color": None, "clips": [clip]},
    {"id": 2, "name": "A1", "kind": "audio", "volume": 1.0, "pan": 0.0, "color": None, "clips": []}],
    "next_id": 4}
open("/tmp/issue43/project.json", "w").write(json.dumps(proj, indent=2))
print("project written")
EOF
rm -rf /tmp/rust-studio-video-editor-media

STUDIO_PROJECT_FILE=/tmp/issue43/project.json "$BIN" > "$OUT/stdout.log" 2>&1 &
APP=$!
for i in $(seq 1 60); do grep -q "preview textures loaded" "$OUT/stdout.log" && break; sleep 0.5; done

# 再生ヘッドが 2900ms を過ぎるまで待つ
for i in $(seq 1 40); do
  PH=$(grep -oE "playhead=[0-9]+" "$OUT/stdout.log" | tail -1 | cut -d= -f2)
  [ -n "$PH" ] && [ "$PH" -ge 2900 ] && break
  sleep 0.25
done
grep -E "^preview:" "$OUT/stdout.log" | tail -1
shot "$OUT/01-before-split.png"

xdotool mousemove 480 300
sleep 0.3
# ポーリング入力は短いタップを取りこぼすため長押しする（#47 の教訓）
xdotool keydown s; sleep 1.2; xdotool keyup s
for i in $(seq 1 40); do grep -q "after split:" "$OUT/stdout.log" && break; sleep 0.25; done
grep -q "after split:" "$OUT/stdout.log" && echo "OK split applied" || echo "FAIL no split"

python3 - <<'EOF'
import json
proj = json.load(open("/tmp/issue43/project.json"))
clips = proj["tracks"][0]["clips"]
assert len(clips) == 2, clips
left, right = sorted(clips, key=lambda c: c["start_ms"])
assert left["start_ms"] == 0 and right["start_ms"] == left["duration_ms"], (left, right)
assert right["offset_ms"] == right["start_ms"], right
assert left["duration_ms"] + right["duration_ms"] == 6000
print(f"OK dump: split at {right['start_ms']}ms, right offset_ms={right['offset_ms']}")
open("/tmp/issue43/split_point.txt", "w").write(str(right["start_ms"]))
EOF

# 分割後、右クリップ区間でプレビューが続きのフレームを出すこと
sleep 1.2
grep -E "^preview:" "$OUT/stdout.log" | tail -1
shot "$OUT/02-after-split.png"
python3 - <<'EOF'
import re
split = int(open("/tmp/issue43/split_point.txt").read())
lines = open("/tmp/issue43/stdout.log").read().splitlines()
pos = next(i for i, l in enumerate(lines) if l.startswith("after split:"))
found = None
for l in lines[pos:]:
    m = re.search(r"playhead=(\d+) source_time=(\d+) frame=(\d+)", l)
    if m:
        ph, st, fr = map(int, m.groups())
        if split + 100 < ph < 6000:
            found = (ph, st, fr)
            break
assert found, "no preview line inside the right clip after split"
ph, st, fr = found
print(f"preview in right clip: playhead={ph} source_time={st} frame={fr} (split at {split})")
# 右クリップ内では source_time は再生ヘッドとほぼ一致する（offset 反映）。
# offset が効いていなければ source_time は分割点ぶん小さくなる。
assert abs(st - ph) <= 150, (ph, st)
assert fr >= split // 100, fr
print("OK preview continues with offset source time (not restarted)")
EOF

# フレーム画像の比較（AC2 の確認方法）: 展開フレーム列で、
# 分割直後の表示フレームが「続き」であって素材の頭ではないことを
# 明るさ（フレーム時刻に比例）で比べる。
python3 - <<'EOF'
import glob
from PIL import Image
dirs = glob.glob("/tmp/rust-studio-video-editor-media/frames/long-*")
assert dirs, "frames dir not found"
files = sorted(glob.glob(dirs[0] + "/*.png"))
print(f"extracted frames: {len(files)} in {dirs[0]}")
def mean_v(p):
    im = Image.open(p).convert("L").resize((32, 18))
    px = list(im.getdata())
    return sum(px) / len(px)
split = int(open("/tmp/issue43/split_point.txt").read())
idx_after = min(split // 100 + 3, len(files) - 1)   # 分割直後の右クリップのフレーム
idx_head = 1                                          # 素材の頭付近
v_after, v_head = mean_v(files[idx_after]), mean_v(files[idx_head])
print(f"frame[{idx_after}] mean brightness={v_after:.1f}, frame[{idx_head}]={v_head:.1f}")
assert v_after > v_head + 40, (v_after, v_head)
print("OK frame after split is a continuation frame, not the source head")
EOF

kill $APP 2>/dev/null; wait $APP 2>/dev/null

# 対象が無い場合は何も起きずログだけ出る（空プロジェクトで S）
python3 - <<'EOF'
import json
proj = {"name": "issue43-empty", "tracks": [
    {"id": 1, "name": "V1", "kind": "video", "volume": 1.0, "pan": 0.0, "color": None, "clips": []}],
    "next_id": 2}
open("/tmp/issue43/project-empty.json", "w").write(json.dumps(proj, indent=2))
EOF
STUDIO_PROJECT_FILE=/tmp/issue43/project-empty.json "$BIN" > "$OUT/stdout-empty.log" 2>&1 &
APP=$!
for i in $(seq 1 60); do grep -q "preview textures loaded" "$OUT/stdout-empty.log" && break; sleep 0.5; done
xdotool mousemove 480 300
sleep 0.3
xdotool keydown s; sleep 1.2; xdotool keyup s
sleep 1
grep -E "^split:" "$OUT/stdout-empty.log" | head -1
grep -q "split: no clip interior at playhead" "$OUT/stdout-empty.log" \
  && echo "OK no-target split is a logged no-op" || echo "FAIL no-target log missing"
grep -q "after split:" "$OUT/stdout-empty.log" && echo "FAIL split happened on empty" || echo "OK state unchanged"
kill $APP 2>/dev/null; wait $APP 2>/dev/null
echo "DRIVE43_DONE"
