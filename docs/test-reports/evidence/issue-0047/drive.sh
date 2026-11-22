#!/bin/bash
# Issue #47 証拠ドライブ: メディアビンの一覧表示、再読み込み
# （R キー）、ダブルクリック配置を実アプリで確認する。
# メディアディレクトリは /tmp/rust-studio-video-editor-media（media_dir）。
# Xvfb(:92) + lavapipe 前提（実ディスプレイ未検証）。
# 注: クリックは保持式（mousedown→sleep→mouseup）にする。
# xdotool click の短い押下は、ポーリング型の入力に取りこぼされる。
set -u
export DISPLAY=:92
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN="$(cd "$(dirname "$0")/../../../.." && pwd)/target/debug/video-editor"
OUT=/tmp/issue47
MED=/tmp/rust-studio-video-editor-media
mkdir -p "$OUT"

shot() {
  ffmpeg -y -loglevel error -f x11grab -draw_mouse 0 -video_size 960x720 -i :92 -frames:v 1 "$1"
}
focus() {
  local wid
  wid=$(xdotool search --name "video-editor" | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  sleep 0.3
}
wait_app() {
  for i in $(seq 1 60); do grep -q "^elements:" "$1" && break; sleep 0.5; done
  for i in $(seq 1 60); do
    shot "$OUT/wait.png"
    python3 -c "
from PIL import Image
im = Image.open('$OUT/wait.png').convert('RGB')
exit(0 if im.getpixel((10, 12)) != (0, 0, 0) else 1)
" && break
    sleep 0.5
  done
}
bin_lines() { grep -c "^media bin:" "$1"; }
hold_click() { # x y
  xdotool mousemove "$1" "$2" mousedown 1; sleep 0.2; xdotool mouseup 1
}

# --- フェーズ1: 起動して一覧を確認（デモプロジェクト）
"$BIN" > "$OUT/stdout.log" 2>&1 &
APP=$!
wait_app "$OUT/stdout.log"
grep -E "^media bin:" "$OUT/stdout.log"
grep -E "^media bin:" "$OUT/stdout.log" | grep -q "opening.mp4" && echo "OK bin lists opening.mp4"
grep -E "^media bin:" "$OUT/stdout.log" | grep -q "bgm.wav" \
  && echo "FAIL wav listed in bin" || echo "OK wav not in bin (video/image only)"
shot "$OUT/01-bin.png"

# --- 素材を追加して R キーで再読み込み（bin 行が増えるまで再試行）
# 実尺 3 秒（既定の新規クリップ長 2000ms と区別できるように）
ffmpeg -y -loglevel error -f lavfi -i testsrc=duration=3:size=320x180:rate=30 -c:v libx264 -pix_fmt yuv420p "$MED/extra.mp4"
# フレーム展開が終わるまで待つ（展開中はフレーム間隔が長く、
# ポーリング型のキー入力を取りこぼしやすい）
for i in $(seq 1 60); do grep -q "preview textures loaded" "$OUT/stdout.log" && break; sleep 0.5; done
BEFORE=$(bin_lines "$OUT/stdout.log")
focus
for try in 1 2 3 4 5 6 7 8; do
  xdotool keydown r; sleep 0.2; xdotool keyup r
  sleep 0.6
  [ "$(bin_lines "$OUT/stdout.log")" -gt "$BEFORE" ] && break
done
grep -E "^media bin:" "$OUT/stdout.log" | tail -1
grep -E "^media bin:" "$OUT/stdout.log" | tail -1 | grep -q "extra.mp4" \
  && echo "OK reload picked up extra.mp4" || echo "FAIL reload did not list extra.mp4"
shot "$OUT/02-reloaded.png"
kill $APP 2>/dev/null; wait $APP 2>/dev/null

# --- フェーズ2: 空きのあるプロジェクトでダブルクリック配置を確認
# V1 は 6000..8000 のみ、A1 は空。再生ヘッドは起動直後は 0 付近で
# 空き領域にあるうちに配置する。extra.mp4 の実尺は 3 秒。
python3 - <<'EOF'
import json
clip = lambda i, name, start, dur, src: {
    "id": i, "name": name, "start_ms": start, "duration_ms": dur, "source": src,
    "offset_ms": 0, "gain": 1.0, "cutout": False, "pitch_semitones": 0.0,
    "color": None, "fade_in_ms": 0, "fade_out_ms": 0}
proj = {"name": "issue47", "tracks": [
    {"id": 1, "name": "V1", "kind": "video", "volume": 1.0, "pan": 0.0, "color": None,
     "clips": [clip(4, "filler", 6000, 2000, "demo/opening.mp4")]},
    {"id": 2, "name": "A1", "kind": "audio", "volume": 1.0, "pan": 0.0, "color": None,
     "clips": []}],
    "next_id": 5}
open("/tmp/issue47/project.json", "w").write(json.dumps(proj, indent=2))
print("project written")
EOF
STUDIO_PROJECT_FILE=/tmp/issue47/project.json "$BIN" > "$OUT/stdout2.log" 2>&1 &
APP=$!
# ウィンドウは model() の prepare_media の後に作られる。前アプリの
# 残像で wait_app が即通過するとクリックを取りこぼすため、
# テクスチャ読み込み完了（= model() 完了）まで待つ。
for i in $(seq 1 60); do grep -q "preview textures loaded" "$OUT/stdout2.log" && break; sleep 0.5; done
sleep 0.5
focus
# パネル上端はトラック数で決まる（2トラック: panel_top=180 +
# panel_height=72 + gap=12 → 264）。ヘッダー 24px、行 20px。
# 一覧は名前順: bg.mp4, extra.mp4, interview.mp4, opening.mp4
# → extra.mp4 は index 1。
ROW_Y=$((264 + 24 + 1 * 20 + 10))
echo "double-click extra.mp4 row at y=$ROW_Y"
hold_click 100 $ROW_Y
sleep 0.15
hold_click 100 $ROW_Y
sleep 0.8
grep -E "^media bin placed:" "$OUT/stdout2.log"
grep -q "media bin placed: extra.mp4" "$OUT/stdout2.log" \
  && echo "OK placed via double-click" || echo "FAIL placement log missing"
grep -E "project after edit" "$OUT/stdout2.log" | grep -o '"name": "extra"[^}]*' | head -1
shot "$OUT/03-placed.png"

kill $APP 2>/dev/null
wait $APP 2>/dev/null
rm -f "$MED/extra.mp4"
echo "DRIVE47_DONE"
