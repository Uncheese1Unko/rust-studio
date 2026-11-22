#!/bin/bash
# Issue #50 証拠ドライブ: T キーでタイトル入力モードに入り、
# 英数字を打って Enter で確定し、プレビュー下部中央にタイトルが
# 表示されることを確認する。素材は暗い単色動画（白文字が
# ピクセルで判別できるように）。
# Xvfb(:92) + lavapipe 前提（実ディスプレイ未検証）。
# クリック・キーは保持式（ポーリング型入力の取りこぼし対策）。
set -u
export DISPLAY=:92
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN="$(cd "$(dirname "$0")/../../../.." && pwd)/target/debug/video-editor"
OUT=/tmp/issue50
MED=/tmp/rust-studio-video-editor-media
mkdir -p "$OUT" "$MED"

shot() {
  ffmpeg -y -loglevel error -f x11grab -draw_mouse 0 -video_size 960x720 -i :92 -frames:v 1 "$1"
}
focus() {
  local wid
  wid=$(xdotool search --name "video-editor" | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  sleep 0.3
}
# プレビュー下部帯（タイトル表示位置）の白ピクセル数
white_count() {
  python3 - "$1" <<'EOF'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
# preview: x 500..940, y 150..398。タイトル帯は下部中央 y 362..394
n = 0
for y in range(360, 396):
    for x in range(540, 900):
        r, g, b = im.getpixel((x, y))
        if r > 240 and g > 240 and b > 240:
            n += 1
print(n)
EOF
}

# 暗い単色の素材（8秒）とプロジェクトを用意する
ffmpeg -y -loglevel error -f lavfi -i color=c=0x202020:duration=8:size=320x180:rate=30 -c:v libx264 -pix_fmt yuv420p "$MED/scene.mp4"
python3 - <<'EOF'
import json
clip = lambda i, name, start, dur, src: {
    "id": i, "name": name, "start_ms": start, "duration_ms": dur, "source": src,
    "offset_ms": 0, "gain": 1.0, "cutout": False, "pitch_semitones": 0.0,
    "color": None, "fade_in_ms": 0, "fade_out_ms": 0, "title_text": ""}
proj = {"name": "issue50", "tracks": [
    {"id": 1, "name": "V1", "kind": "video", "volume": 1.0, "pan": 0.0, "color": None,
     "clips": [clip(4, "scene", 0, 8000, "scene.mp4")]},
    {"id": 2, "name": "A1", "kind": "audio", "volume": 1.0, "pan": 0.0, "color": None,
     "clips": []}],
    "next_id": 5}
open("/tmp/issue50/project.json", "w").write(json.dumps(proj, indent=2))
print("project written")
EOF

STUDIO_PROJECT_FILE=/tmp/issue50/project.json "$BIN" > "$OUT/stdout.log" 2>&1 &
APP=$!
for i in $(seq 1 60); do grep -q "preview textures loaded" "$OUT/stdout.log" && break; sleep 0.5; done
sleep 0.5
focus
shot "$OUT/01-before.png"
W0=$(white_count "$OUT/01-before.png")
echo "white pixels in title strip before: $W0"

# T キー（離しエッジで開始）→ 入力モード
xdotool keydown t; sleep 0.25; xdotool keyup t; sleep 0.6
grep -E "title input begin" "$OUT/stdout.log" && echo "OK title input began" || echo "FAIL no begin log"
# タイトルを打つ（保持式に1文字ずつ）
for k in O p 3 n; do xdotool keydown "$k"; sleep 0.12; xdotool keyup "$k"; sleep 0.1; done
sleep 0.5
shot "$OUT/02-typing.png"
# Enter で確定
xdotool keydown Return; sleep 0.2; xdotool keyup Return; sleep 0.8
# ダンプは複数行の pretty JSON なので、ログ全体から探す
grep -o '"title_text": "[^"]*"' "$OUT/stdout.log" | sort | uniq -c
grep -q '"title_text": "Op3n"' "$OUT/stdout.log" \
  && echo "OK title committed as Op3n" || echo "FAIL title not in dump"
shot "$OUT/03-titled.png"
W1=$(white_count "$OUT/03-titled.png")
echo "white pixels in title strip after: $W1"
python3 - "$W0" "$W1" <<'EOF'
import sys
w0, w1 = int(sys.argv[1]), int(sys.argv[2])
assert w1 > w0 + 100, (w0, w1)
print(f"OK title visible in preview ({w0} -> {w1} white px)")
EOF

# Esc での中止も確認する: T → 打鍵 → Esc で変わらないこと
xdotool keydown t; sleep 0.25; xdotool keyup t; sleep 0.5
xdotool keydown X; sleep 0.12; xdotool keyup X; sleep 0.2
xdotool keydown Escape; sleep 0.2; xdotool keyup Escape; sleep 0.5
grep -E "title input cancelled" "$OUT/stdout.log" && echo "OK esc cancels" || echo "FAIL esc log missing"
grep -q 'Op3nX' "$OUT/stdout.log" \
  && echo "FAIL cancelled input leaked" || echo "OK cancelled input not applied"

kill $APP 2>/dev/null
wait $APP 2>/dev/null
rm -f "$MED/scene.mp4"
echo "DRIVE50_DONE"
