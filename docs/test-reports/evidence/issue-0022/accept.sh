#!/bin/bash
# Issue #22 受け入れ: GUI で K キーを押し、MODNet のオフライン処理が
# 進捗表示つきで走り、合成プレビューに切り替わることを確認する。
set -u
BIN="$HOME/workspace/repos/rust-studio/target/debug/video-editor"
W=/tmp/cutout22
rm -rf "$W"; mkdir -p "$W"
export STUDIO_MODNET_MODEL=/home/hatch/cutout/modnet.onnx
export ORT_DYLIB_PATH="$HOME/.local/lib/python3.12/site-packages/onnxruntime/capi/libonnxruntime.so.1.32.0"
# 前回のメディア（フレーム列・マスク列）が残っていると証拠が混ざるので消す
MEDIA=/tmp/rust-studio-video-editor-media
rm -rf "$MEDIA"

Xvfb :99 -screen 0 1024x600x24 > "$W/xvfb.log" 2>&1 &
XVFB=$!
sleep 2
DISPLAY=:99 "$BIN" > "$W/gui-stdout.txt" 2>&1 &
APP=$!
cleanup() { kill $APP $XVFB 2>/dev/null; }
trap cleanup EXIT

# interview クリップがプレビューに出るまで待つ
for i in $(seq 1 60); do
  grep -q "preview: source=demo/interview.mp4" "$W/gui-stdout.txt" && break
  sleep 1
done
grep -c "preview: source=demo/interview.mp4" "$W/gui-stdout.txt" || true
DISPLAY=:99 ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$W/shot1-before.png"
# K キーで切り抜きを有効化（ポーリングで拾えるよう keydown を保持する）
DISPLAY=:99 xdotool mousemove 480 300
sleep 0.3
DISPLAY=:99 xdotool keydown k
sleep 1
DISPLAY=:99 xdotool keyup k
# 処理開始の記録を待つ
for i in $(seq 1 20); do
  grep -q "cutout started" "$W/gui-stdout.txt" && break
  sleep 1
done
grep "cutout" "$W/gui-stdout.txt" | head -5
sleep 5
DISPLAY=:99 ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$W/shot2-progress.png"
# 完了を待つ（60 フレーム想定。長めに待つ）
for i in $(seq 1 40); do
  grep -q "cutout done" "$W/gui-stdout.txt" && break
  sleep 5
done
grep "cutout" "$W/gui-stdout.txt" | tail -3
sleep 6
DISPLAY=:99 ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$W/shot3-composite.png"
echo done
