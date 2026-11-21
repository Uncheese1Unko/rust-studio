#!/bin/bash
# Issue #57 acceptance drive: video-editor GUI under Xvfb + lavapipe.
# 映像トラック追加（+ VIDEO）→ 音声トラック追加（+ AUDIO）→ リネーム
# （V3 → SUB）→ SUB 行へクリップ配置 → V2 削除（bg クリップごと）→
# プレビューが SUB 行のクリップをアクティブなレイヤーとして扱うことを
# stdout の preview 行で確認する。
# クリックはアプリのポーリングが拾えるよう down/up を分けて送る
# （#14 の方式）。撮影は -draw_mouse 0（カーソルを合成しない）。
set -u
export DISPLAY=:99
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=${1:-/tmp/ve57}
mkdir -p "$OUT"
Xvfb :99 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
"$ROOT/target/debug/video-editor" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
# 初回フレームの描画が始まるまで待つ（テクスチャ読み込みと lavapipe の
# 初期化で、起動直後は黒い画面のことがある。黒い間はクリックが届かない）
for i in $(seq 1 90); do
  ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/probe.png" 2>/dev/null
  python3 -c "
from PIL import Image
import sys
img = Image.open('$OUT/probe.png').convert('RGB')
sys.exit(0 if img.getpixel((10, 12)) != (0, 0, 0) else 1)
" && break
  sleep 1
done
sleep 1
shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :99 -frames:v 1 "$OUT/$1"; }
click() { # x y
  xdotool mousemove "$1" "$2"; sleep 0.2
  xdotool mousedown 1; sleep 0.4; xdotool mouseup 1; sleep 0.6
}
dblclick() { # x y
  xdotool mousemove "$1" "$2"; sleep 0.2
  xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.1
  xdotool mousedown 1; sleep 0.15; xdotool mouseup 1; sleep 0.6
}
shot shot1-initial.png
# 映像トラック追加: 追加行ヘッダー上半分（x=60, y=180）
click 60 180
shot shot2-video-added.png
# 音声トラック追加: 追加行は1行下がり、下半分の中心は y=252
click 60 252
shot shot3-audio-added.png
# リネーム: V3（4行目、中心 y=192）のヘッダーをダブルクリック
dblclick 60 192
xdotool key BackSpace; sleep 0.2; xdotool key BackSpace; sleep 0.2; xdotool key BackSpace; sleep 0.2
xdotool type "SUB"
sleep 0.6
shot shot4-renaming.png
xdotool key Return
sleep 1
shot shot5-renamed.png
# SUB 行の空き場所（x=300 → 約2143ms）をダブルクリックしてクリップ配置
dblclick 300 192
sleep 0.5
shot shot6-clip-placed.png
# V2（3行目、中心 y=144）の × をクリックして bg クリップごと削除
click 111 144
shot shot7-v2-removed.png
# プレビュー確認: 再生ヘッドが SUB のクリップ区間（2143〜2500ms は
# 他の映像クリップが無い）に入ると、preview 行に dropped.mp4 が出る
for i in $(seq 1 40); do grep -q "preview: source=demo/dropped.mp4" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
grep -m 1 "preview: source=demo/dropped.mp4" "$OUT/gui-stdout.txt" | sed 's/^/preview-evidence: /'
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys
from PIL import Image
out = sys.argv[1]
def px(path, x, y):
    return Image.open(path).convert("RGB").getpixel((x, y))
# 4行目の帯（x=400, y=192）: 初期は背景、V3 追加後は行の色（交互色 index3）
print("shot1 row4 band:", px(f"{out}/shot1-initial.png", 400, 192), "expect ~(23, 23, 28)")
print("shot2 row4 band:", px(f"{out}/shot2-video-added.png", 400, 192), "expect ~(36, 36, 41)")
PYEOF
grep -c "after track add" "$OUT/gui-stdout.txt" | sed 's/^/after track add count: /'
grep -o "rename begin.*" "$OUT/gui-stdout.txt"
grep -o '"name": "SUB"' "$OUT/gui-stdout.txt" | head -1
grep -c "placed:" "$OUT/gui-stdout.txt" | sed 's/^/placed count: /'
grep -c "after track remove" "$OUT/gui-stdout.txt" | sed 's/^/after track remove count: /'
grep -c '"name": "bg"' "$OUT/gui-stdout.txt" | sed 's/^/bg clip mentions (initial dump only=1 expected after removal): /'
