#!/usr/bin/env bash
# Issue #89 の駆動証拠: CLI 取り込み→書き出し WAV の測定、MCP での
# サンプラー差し替え回帰、GUI で取り込み結果を開いて再生が進むことと
# ピアノロール見出しに既定シンセが表示されることの確認。
# 使い方: bash drive.sh [REPO_ROOT]
# 前提: xdotool / ffmpeg / python3、DISPLAY はスクリプトが :96 に
# Xvfb を立てる（lavapipe）。実ディスプレイ未検証。
set -u
ROOT="${1:-$(cd "$(dirname "$0")/../../../.." && pwd)}"
HERE="$(cd "$(dirname "$0")" && pwd)"
DAW="$ROOT/target/debug/daw"
MCP="$ROOT/target/debug/studio-mcp"
SEED="$ROOT/docs/test-reports/evidence/issue-0070/project.seed.json"
MID="$ROOT/docs/test-reports/evidence/issue-0070/song.mid"
export DISPLAY=:96
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
W=/tmp/issue0089
mkdir -p "$W"
# 全出力を証拠ログにも残す（CLI／MCP 区間の測定結果を後から照合できる
# ように。レビュー round 1 の Minor 指摘への対応）
exec > >(tee "$HERE/drive-log.txt") 2>&1

echo "== CLI: import -> dump -> export -> measure =="
cp "$SEED" "$W/project.json"
"$DAW" --import-midi "$W/project.json" "$MID" | grep "midi imported" \
  || { echo "FAIL: cli import"; exit 1; }
python3 - "$W/project.json" << 'PYEOF' || exit 1
import json, sys
p = json.load(open(sys.argv[1]))
for t in p["tracks"]:
    assert t["instrument"] == {"type": "synth", "waveform": "triangle"}, t
print("dump: both tracks carry synth/triangle")
PYEOF
"$DAW" --export-wav "$W/project.json" "$W/out.wav" || exit 1
python3 "$HERE/measure_wav.py" "$W/out.wav" || exit 1

echo "== MCP: replace Lead with a sampler still sounds =="
cp "$W/project.json" "$W/project2.json"
python3 "$HERE/mcp_replace.py" "$MCP" || exit 1
"$DAW" --export-wav "$W/project2.json" "$W/out2.wav" || exit 1
python3 "$HERE/measure_replace.py" "$W/out2.wav" || exit 1

echo "== GUI: open imported project, playback advances, piano roll header =="
Xvfb :96 -screen 0 960x540x24 > "$W/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 2
STUDIO_PROJECT_FILE="$W/project.json" "$DAW" \
  > "$HERE/gui-stdout.txt" 2> "$HERE/gui-stderr.txt" &
APP_PID=$!
for i in $(seq 1 60); do
  grep -q "elements:" "$HERE/gui-stdout.txt" 2>/dev/null && break; sleep 0.5
done
grep -q "project file mode: $W/project.json" "$HERE/gui-stdout.txt" \
  || { echo "FAIL: project file not loaded"; kill -9 $APP_PID $XVFB_PID 2>/dev/null; exit 1; }
snap() {
  ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 \
    -i :96 -frames:v 1 "$1"
}
focus_win() {
  local wid
  wid=$(xdotool search --name "^daw$" 2>/dev/null | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  xdotool mousemove 60 48; sleep 0.2; xdotool mousedown 1; sleep 0.3
  xdotool mouseup 1; sleep 0.4
}
focus_win
snap "$HERE/shot1-playing-a.png"
sleep 0.7
snap "$HERE/shot2-playing-b.png"
python3 - "$HERE/shot1-playing-a.png" "$HERE/shot2-playing-b.png" << 'PYEOF' || exit 1
# 再生中はトランスポート表示（進行バー・時刻・レベル）と再生ヘッドが
# 毎フレーム変わる。0.7 秒間隔の2枚で、トランスポート帯と
# タイムライン帯の差分ピクセル数が十分あることを進行の証拠とする
# （静止した画面は決定的レンダリングで差分 0 になる。目視でも
# BEAT・時刻表示・レベル値の変化をスクリーンショットで確認する）。
from PIL import Image
import sys
a = Image.open(sys.argv[1]).convert('RGB')
b = Image.open(sys.argv[2]).convert('RGB')
def diff_count(y0, y1):
    n = 0
    for y in range(y0, y1):
        for x in range(0, 960):
            if a.getpixel((x, y)) != b.getpixel((x, y)):
                n += 1
    return n
t = diff_count(255, 330)   # 01 TRANSPORT 帯
l = diff_count(0, 150)     # 02 TIMELINE 帯（再生ヘッド）
print(f"diff pixels: transport={t} timeline={l}")
assert t > 500 and l > 200, "screen static: playback not advancing"
print("playback advancing OK")
PYEOF
# Lead クリップをダブルクリックしてピアノロールを開く（#70 の実測座標。
# 押し 0.12 秒・間隔 0.08 秒の詰めたタイミングで送る。0.25 秒押しだと
# ダブルクリック判定の窓を超えることを実測した）
opened=0
for i in 1 2 3 4 5; do
  xdotool mousemove 182 57; sleep 0.15
  xdotool mousedown 1; sleep 0.12; xdotool mouseup 1; sleep 0.08
  xdotool mousedown 1; sleep 0.12; xdotool mouseup 1; sleep 0.6
  grep -q 'piano roll opened:' "$HERE/gui-stdout.txt" && { opened=1; break; }
done
[ "$opened" = 1 ] \
  || { echo "FAIL: piano roll did not open"; kill -9 $APP_PID $XVFB_PID 2>/dev/null; exit 1; }
sleep 1
snap "$HERE/shot3-piano-roll.png"
xdotool keydown Escape; sleep 0.6; xdotool keyup Escape; sleep 0.5
kill $APP_PID 2>/dev/null; kill $XVFB_PID 2>/dev/null
echo "DRIVE OK"
