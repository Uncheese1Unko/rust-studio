#!/bin/bash
# Issue #46 証拠ドライブ: Ctrl+ホイールでズーム、通常ホイールで
# スクロールし、描画の変化とプロジェクト JSON の不変を確認する。
# X11 ではホイール1クリックが press/release の2イベントになるため、
# Ctrl+ホイール2クリックで 1.25^4 ≒ 2.44 倍になる（DAW #38 の知見）。
# Xvfb(:92) + lavapipe 前提（実ディスプレイ未検証）。
set -u
cd "$(dirname "$0")/../../../.."
export DISPLAY=:92
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=target/debug/video-editor
OUT=/tmp/issue46
mkdir -p "$OUT"

# プロジェクトファイルモードで起動し、ズーム・スクロールしても
# ファイルが変わらないことを確かめる（AC4）。
python3 - <<'EOF'
import json
proj = {
  "name": "issue46",
  "tracks": [
    {"id": 1, "name": "V1", "kind": "video", "volume": 1.0, "pan": 0.0, "color": None, "clips": [
      {"id": 4, "name": "opening", "start_ms": 0, "duration_ms": 2000, "source": "demo/opening.mp4",
       "offset_ms": 0, "gain": 1.0, "cutout": False, "pitch_semitones": 0.0, "color": None,
       "fade_in_ms": 0, "fade_out_ms": 0},
      {"id": 5, "name": "interview", "start_ms": 2500, "duration_ms": 4000, "source": "demo/interview.mp4",
       "offset_ms": 0, "gain": 1.0, "cutout": False, "pitch_semitones": 0.0, "color": None,
       "fade_in_ms": 0, "fade_out_ms": 0}
    ]},
    {"id": 2, "name": "A1", "kind": "audio", "volume": 1.0, "pan": 0.0, "color": None, "clips": [
      {"id": 6, "name": "bgm", "start_ms": 0, "duration_ms": 8000, "source": "demo/bgm.wav",
       "offset_ms": 0, "gain": 1.0, "cutout": False, "pitch_semitones": 0.0, "color": None,
       "fade_in_ms": 0, "fade_out_ms": 0}
    ]}
  ],
  "next_id": 7
}
open("/tmp/issue46/project.json", "w").write(json.dumps(proj, indent=2))
print("project written")
EOF
cp "$OUT/project.json" "$OUT/project.orig.json"

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

STUDIO_PROJECT_FILE=/tmp/issue46/project.json "$BIN" > "$OUT/stdout.log" 2>&1 &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$OUT/stdout.log" && break; sleep 0.5; done
for i in $(seq 1 60); do
  shot "$OUT/wait.png"
  [ "$(px "$OUT/wait.png" 10 12)" != "0,0,0" ] && break
  sleep 0.5
done
shot "$OUT/01-initial.png"
echo "launched"
BEFORE=$(px "$OUT/01-initial.png" 450 48)
echo "pixel(450,48) before zoom: $BEFORE (interview face expected)"

# 1) Ctrl+ホイール（マウスは x=204 = 1000ms 付近）でズームイン
focus
xdotool mousemove 204 48
xdotool keydown ctrl
xdotool click 4
sleep 0.4
xdotool click 4
sleep 0.4
xdotool keyup ctrl
sleep 0.5
grep -E "^zoom:" "$OUT/stdout.log" | tail -2
ZOOM=$(grep -E "^zoom:" "$OUT/stdout.log" | tail -1 | sed 's/zoom: \([0-9.]*\).*/\1/')
python3 - "$ZOOM" <<'EOF'
import sys
z = float(sys.argv[1])
assert z > 2.0, z
print(f"OK zoomed to {z}")
EOF
shot "$OUT/02-zoomed.png"
AFTER=$(px "$OUT/02-zoomed.png" 450 48)
echo "pixel(450,48) after zoom: $AFTER (row background expected)"
[ "$BEFORE" != "$AFTER" ] && echo "OK drawing changed with zoom" || echo "FAIL drawing unchanged"

# 2) 通常ホイール下でスクロール（offset が増える）、上で戻る
xdotool click 5; sleep 0.3; xdotool click 5; sleep 0.5
grep -E "^scroll offset:" "$OUT/stdout.log" | tail -2
OFF1=$(grep -E "^scroll offset:" "$OUT/stdout.log" | tail -1 | sed 's/scroll offset: //')
xdotool click 4; sleep 0.3; xdotool click 4; sleep 0.3; xdotool click 4; sleep 0.5
OFF2=$(grep -E "^scroll offset:" "$OUT/stdout.log" | tail -1 | sed 's/scroll offset: //')
echo "offset after down=$OFF1 after up=$OFF2"
python3 - "$OFF1" "$OFF2" <<'EOF'
import sys
off1, off2 = int(sys.argv[1]), int(sys.argv[2])
assert off1 > 0, off1
assert off2 < off1 and off2 >= 0, (off1, off2)
print(f"OK scroll: 0 -> {off1} -> {off2} (never negative)")
EOF
shot "$OUT/03-scrolled.png"

# 3) プレビューは再生ヘッド時刻で選ばれ続ける（ズーム・スクロールの影響なし）
grep -c "^preview:" "$OUT/stdout.log"
grep -q "^preview:" "$OUT/stdout.log" && echo "OK preview lines present after zoom/scroll"

# 4) プロジェクトファイルは変わっていない（倍率・スクロールは保存されない）
if cmp -s "$OUT/project.json" "$OUT/project.orig.json"; then
  echo "OK project file unchanged"
else
  echo "FAIL project file changed"
fi

kill $APP 2>/dev/null
wait $APP 2>/dev/null
echo "DRIVE46_DONE"
