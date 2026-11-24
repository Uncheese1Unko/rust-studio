#!/bin/bash
# Issue #81 証拠ドライブ: GUI で L キーからスクリプトエフェクトを
# 設定し、プレビューに反転が反映されること、P キーでパラメータが
# ダンプに出ること、壊れたスクリプトでエラー表示になりアプリが
# 落ちないこと、プロジェクトファイルへ保存されることを確認する。
# Xvfb(:96) + lavapipe 前提（実ディスプレイ未検証）。
set -u
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=/tmp/issue81
MED=/tmp/rust-studio-video-editor-media
mkdir -p "$OUT" "$MED/scripts"
export DISPLAY=:96
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=$ROOT/target/debug/video-editor

cat > "$MED/scripts/invert.lua" <<'EOF'
for y = 0, frame.height - 1 do
  for x = 0, frame.width - 1 do
    local r, g, b, a = frame.get(x, y)
    frame.set(x, y, 255 - r, 255 - g, 255 - b, a)
  end
end
EOF
cat > "$MED/scripts/bad.lua" <<'EOF'
error("drive bad script")
EOF
rm -rf "$MED/scriptfx"

cat > "$OUT/project.json" <<'EOF'
{"name": "issue81 drive", "tracks": [
 {"id": 1, "name": "V1", "kind": "video", "clips": [{"id": 3, "name": "opening", "start_ms": 0, "duration_ms": 4000, "source": "demo/opening.mp4", "gain": 1.0}], "volume": 1.0, "pan": 0.0}
], "next_id": 5}
EOF

Xvfb :96 -screen 0 1024x768x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE=$OUT/project.json "$BIN" > "$OUT/app.log" 2>&1 &
APP=$!
cleanup() { kill $APP 2>/dev/null; kill $XVFB_PID 2>/dev/null; }
trap cleanup EXIT

for i in $(seq 1 60); do grep -q "^preview:" "$OUT/app.log" && break; sleep 0.5; done
grep -q "^preview:" "$OUT/app.log" || { echo "FAIL: no preview lines"; exit 1; }
WID=$(xdotool search --name "video-editor" | head -1)
xdotool windowfocus "$WID" 2>/dev/null; xdotool windowactivate "$WID" 2>/dev/null
sleep 2

shot() { ffmpeg -y -loglevel error -f x11grab -draw_mouse 0 -video_size 960x720 -i :96 -frames:v 1 "$1"; }
# ポーリング型入力の取りこぼし対策として保持式で送る（#50 の知見）。
tap() { xdotool keydown "$1"; sleep 0.2; xdotool keyup "$1"; sleep 0.3; }
type_str() {
  local s="$1" i ch key
  for (( i=0; i<${#s}; i++ )); do
    ch="${s:i:1}"
    case "$ch" in
      .) key=period ;;
      =) key=equal ;;
      -) key=minus ;;
      _) key=underscore ;;
      *) key="$ch" ;;
    esac
    xdotool keydown "$key"; sleep 0.12; xdotool keyup "$key"; sleep 0.1
  done
}
shot "$OUT/base.png"

# --- L キー: invert.lua を設定 ---
tap l
grep -q "script input begin: clip 3" "$OUT/app.log" || { echo "FAIL: script session did not begin"; exit 1; }
type_str "invert.lua"
tap Return; sleep 0.5
grep -A2 '"script_effect": {' "$OUT/app.log" | grep -q '"script": "invert.lua"' || { echo "FAIL: dump lacks script_effect"; exit 1; }
echo "OK dump shows script_effect invert.lua"
for i in $(seq 1 20); do grep -q "^script effect:" "$OUT/app.log" && break; sleep 0.5; done
grep -q "^script effect:" "$OUT/app.log" || { echo "FAIL: no script effect preview line"; exit 1; }
echo "OK preview applied script effect"
sleep 1
shot "$OUT/fx.png"

# プレビュー領域の平均色が、scriptfx 出力ファイルの平均色と近いこと、
# かつそのファイルがベースフレームの反転であることをファイルで照合。
python3 - "$OUT/fx.png" "$MED" <<'EOF'
import glob, sys
from PIL import Image
def avg(path, box=None):
    im = Image.open(path).convert("RGB")
    if box: im = im.crop(box)
    px = list(im.getdata())
    n = len(px)
    return tuple(sum(p[i] for p in px) / n for i in range(3))
shot_avg = avg(sys.argv[1], (500, 150, 940, 398))
fx_files = sorted(glob.glob(sys.argv[2] + "/scriptfx/s*.png"))
assert fx_files, "no scriptfx output files"
fx_avg = avg(fx_files[-1])
print("shot preview avg:", tuple(round(v,1) for v in shot_avg), "scriptfx avg:", tuple(round(v,1) for v in fx_avg))
assert all(abs(a-b) < 30 for a, b in zip(shot_avg, fx_avg)), "preview does not match effected frame"
# ベースフレームの中に反転関係（平均の和がほぼ 255）のものがある
inv = tuple(255 - v for v in fx_avg)
found = False
for f in glob.glob(sys.argv[2] + "/frames/*/f*.png"):
    ba = avg(f)
    if all(abs(a-b) < 12 for a, b in zip(ba, inv)):
        found = True
        print("base frame inverse match:", f)
        break
assert found, "no base frame matches the inverse of the effected frame"
print("OK preview pixels match effected frame; effected frame is inverse of a base frame")
EOF
[ $? -eq 0 ] || exit 1

# --- P キー: パラメータ level=3 を設定 ---
tap p
grep -q "script param input begin: clip 3" "$OUT/app.log" || { echo "FAIL: param session did not begin"; exit 1; }
type_str "level=3"
tap Return; sleep 0.5
grep -A3 '"params": {' "$OUT/app.log" | grep -q '"level": 3.0' || { echo "FAIL: dump lacks param level=3.0"; exit 1; }
echo "OK dump shows params level=3.0"

# --- L キー: bad.lua に差し替え → エラー表示、アプリは落ちない ---
tap l
for i in $(seq 1 10); do tap BackSpace; done
type_str "bad.lua"
tap Return; sleep 1.5
grep -q "script error:" "$OUT/app.log" || { echo "FAIL: no script error line"; exit 1; }
grep "script error:" "$OUT/app.log" | grep -q "drive bad script" || { echo "FAIL: error line lacks script message"; exit 1; }
kill -0 $APP 2>/dev/null || { echo "FAIL: app died after bad script"; exit 1; }
echo "OK script error reported and app alive"
shot "$OUT/bad.png"

# --- プロジェクトファイルへ保存されている（再読込で残る根拠） ---
python3 - "$OUT/project.json" <<'EOF'
import json, sys
p = json.load(open(sys.argv[1]))
clip = p["tracks"][0]["clips"][0]
fx = clip.get("script_effect")
assert fx and fx["script"] == "bad.lua", fx
assert fx["params"].get("level") == 3.0, fx
print("OK project file saved script_effect bad.lua with params", fx["params"])
EOF
[ $? -eq 0 ] || exit 1
echo "DRIVE OK"
