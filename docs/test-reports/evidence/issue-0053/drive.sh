#!/bin/bash
# Issue #53 証拠ドライブ: XDND（#30 で確立した検証方法）でファイルを
# ドロップし、選択規則どおりのトラック＋再生ヘッド位置にクリップが
# 置かれること、対応外は理由付きで拒否されること、置かれた
# クリップがドラッグ移動できることを確認する。
# Xvfb(:92) + lavapipe 前提（実ディスプレイ・実ファイルマネージャ
# での確認は未検証）。
set -u
export DISPLAY=:92
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=~/workspace/repos/rust-studio-lane-b/target/debug/video-editor
XDND=~/workspace/repos/rust-studio-lane-b/docs/test-reports/evidence/issue-0030/xdnd_source.py
OUT=/tmp/issue53
SRC=/tmp/drop53
mkdir -p "$OUT" "$SRC"

[ -S /tmp/.X11-unix/X92 ] || { setsid nohup Xvfb :92 -screen 0 960x720x24 > /tmp/xvfb92.log 2>&1 < /dev/null & sleep 2; }

shot() {
  ffmpeg -y -loglevel error -f x11grab -draw_mouse 0 -video_size 960x720 -i :92 -frames:v 1 "$1"
}

# ドロップ素材: 3秒の動画と、対応外のテキスト
ffmpeg -y -loglevel error -f lavfi -i testsrc2=duration=3:size=320x180:rate=30 -c:v libx264 -pix_fmt yuv420p "$SRC/movie.mp4"
echo "just a note" > "$SRC/notes.txt"

python3 - <<'EOF'
import json
proj = {"name": "issue53", "tracks": [
    {"id": 1, "name": "V1", "kind": "video", "volume": 1.0, "pan": 0.0, "color": None, "clips": []},
    {"id": 2, "name": "A1", "kind": "audio", "volume": 1.0, "pan": 0.0, "color": None, "clips": []}],
    "next_id": 3}
open("/tmp/issue53/project.json", "w").write(json.dumps(proj, indent=2))
print("project written")
EOF

STUDIO_PROJECT_FILE=/tmp/issue53/project.json "$BIN" > "$OUT/stdout.log" 2>&1 &
APP=$!
for i in $(seq 1 60); do grep -q "preview textures loaded" "$OUT/stdout.log" && break; sleep 0.5; done
sleep 0.5
shot "$OUT/01-initial.png"

# 1) 動画をドロップ → V1 の再生ヘッド位置に実尺 3000ms で配置
python3 "$XDND" video-editor 300 100 "file://$SRC/movie.mp4" > "$OUT/xdnd1.log" 2>&1
for i in $(seq 1 60); do grep -q "project after edit" "$OUT/stdout.log" && break; sleep 0.5; done
grep -E "^drop imported:" "$OUT/stdout.log"
grep -q "drop imported: $SRC/movie.mp4" "$OUT/stdout.log" \
  && echo "OK drop imported" || echo "FAIL import log missing"
grep -o '"source": "/tmp/drop53/movie.mp4"' "$OUT/stdout.log" | head -1
grep -A2 '"source": "/tmp/drop53/movie.mp4"' "$OUT/stdout.log" | head -3
python3 - "$OUT/stdout.log" <<'EOF'
import re, sys
text = open(sys.argv[1]).read()
m = re.search(r'"name": "movie",\s*"start_ms": (\d+),\s*"duration_ms": (\d+)', text)
assert m, "movie clip not in dump"
print(f"OK clip in dump: start={m.group(1)} duration={m.group(2)}")
assert m.group(2) == "3000", m.groups()
EOF
sleep 1
shot "$OUT/02-dropped.png"
grep -E "^preview: source=/tmp/drop53/movie.mp4" "$OUT/stdout.log" | tail -1
grep -qE "^preview: source=/tmp/drop53/movie.mp4" "$OUT/stdout.log" \
  && echo "OK preview shows dropped movie" || echo "FAIL preview line missing"

# 2) 対応外ファイルは理由付きで拒否（バースト判定を避けて間隔を空ける）
sleep 2
BEFORE=$(grep -c "drop imported" "$OUT/stdout.log")
python3 "$XDND" video-editor 300 100 "file://$SRC/notes.txt" > "$OUT/xdnd2.log" 2>&1
sleep 1.5
grep -E "^drop rejected:" "$OUT/stdout.log" | tail -1
grep -q "drop rejected: $SRC/notes.txt (unsupported file type" "$OUT/stdout.log" \
  && echo "OK unsupported rejected with reason" || echo "FAIL rejection log missing"
[ "$(grep -c "drop imported" "$OUT/stdout.log")" = "$BEFORE" ] \
  && echo "OK no new clip from txt" || echo "FAIL txt created a clip"

# 3) 置かれたクリップをドラッグ移動できる（通常クリップと同じ）
START=$(python3 -c "
import re
text = open('$OUT/stdout.log').read()
m = re.search(r'\"name\": \"movie\",\s*\"start_ms\": (\d+)', text)
print(m.group(1))
")
CX=$(python3 -c "print(int(120 + ($START + 1500) * 0.084))")
echo "drag dropped clip body from x=$CX (start=$START)"
wid=$(xdotool search --name "video-editor" | head -1)
[ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
sleep 0.3
xdotool mousemove $CX 48 mousedown 1; sleep 0.3
xdotool mousemove $((CX + 42)) 48; sleep 0.3
xdotool mouseup 1
for i in $(seq 1 20); do grep -q "after drag" "$OUT/stdout.log" && break; sleep 0.5; done
python3 - "$OUT/stdout.log" "$START" <<'EOF'
import re, sys
text = open(sys.argv[1]).read()
starts = re.findall(r'"name": "movie",\s*"start_ms": (\d+)', text)
print("movie starts seen:", starts)
assert len(starts) >= 2 and int(starts[-1]) > int(sys.argv[2]), starts
print(f"OK dropped clip moved: {sys.argv[2]} -> {starts[-1]}")
EOF
shot "$OUT/03-moved.png"

kill $APP 2>/dev/null
wait $APP 2>/dev/null
echo "DRIVE53_DONE"
