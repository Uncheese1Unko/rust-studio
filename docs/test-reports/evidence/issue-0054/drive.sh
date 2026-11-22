#!/bin/bash
set -u
export DISPLAY=:92 LIBCLANG_PATH=/usr/lib/llvm-18/lib
REPO=$HOME/workspace/repos/rust-studio-lane-b
EV=$REPO/docs/test-reports/evidence/issue-0054
if [ ! -S /tmp/.X11-unix/X92 ]; then
  setsid nohup Xvfb :92 -screen 0 960x720x24 >/tmp/xvfb92.log 2>&1 &
  sleep 2
fi
rm -f /tmp/export54b/export.mp4
cp /tmp/export54b/project2.json /tmp/export54b/gui-project.json
cd $REPO
STUDIO_PROJECT_FILE=/tmp/export54b/gui-project.json ./target/debug/video-editor > /tmp/drive54-stdout.txt 2>&1 &
APP=$!
for i in $(seq 1 30); do grep -q "preview textures loaded" /tmp/drive54-stdout.txt && break; sleep 1; done
sleep 1
ffmpeg -y -loglevel error -f x11grab -video_size 960x720 -i :92 -frames:v 1 $EV/01-before-export.png
# EXPORT MP4 ボタン（500..670, 474..498）の中心を押す
xdotool mousemove 585 486 sleep 0.3 mousedown 1 sleep 0.4 mouseup 1
# 完了を待つ（180フレームの書き出し）
for i in $(seq 1 60); do grep -q "export done:" /tmp/drive54-stdout.txt && break; sleep 2; done
grep "export" /tmp/drive54-stdout.txt | tail -3
ls -la /tmp/export54b/export.mp4
ffmpeg -y -loglevel error -f x11grab -video_size 960x720 -i :92 -frames:v 1 $EV/02-exported.png
cp /tmp/drive54-stdout.txt $EV/gui-stdout.txt
kill $APP 2>/dev/null; wait $APP 2>/dev/null
echo DRIVE54-DONE
