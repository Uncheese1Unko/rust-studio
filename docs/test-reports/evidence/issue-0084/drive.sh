#!/bin/bash
# Issue #84 証拠ドライブ: 再生ヘッドへの表示追従（ページ送り）、
# F キーでのオン／オフ、ラップアラウンド時のオフセット 0 リセットを
# 実アプリで確認する。Xvfb(:94) + lavapipe 前提（実ディスプレイ未検証）。
set -u
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=/tmp/issue84
mkdir -p "$OUT"
export DISPLAY=:94
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=$ROOT/target/debug/video-editor
cd "$ROOT"

Xvfb :94 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1

shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :94 -frames:v 1 "$1"; }
focus_win() {
  local wid
  wid=$(xdotool search --name "video-editor" 2>/dev/null | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  sleep 0.3
}
wait_drawn() {
  for i in $(seq 1 40); do
    shot "$OUT/probe.png"
    python3 -c "
from PIL import Image
import sys
im = Image.open('$OUT/probe.png').convert('RGB')
sys.exit(0 if im.getpixel((480, 270)) != (0, 0, 0) else 1)
" && break
    sleep 0.5
  done
}
playhead_red_count() {
  python3 - "$1" <<'PYEOF'
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert('RGB')
n = 0
for y in range(24, 120):
    for x in range(115, 960):
        r, g, b = im.getpixel((x, y))
        if r > 180 and g < 110 and b < 110:
            n += 1
print(n)
PYEOF
}
wait_log() { # $1=pattern $2=timeout_s $3=logfile
  for i in $(seq 1 $(( $2 * 4 ))); do
    grep -q "$1" "$3" && return 0
    sleep 0.25
  done
  return 1
}

# ---- Launch A: 終端 21,000ms。ページ送りと F オフを確認 ----
cat > "$OUT/projectA.json" <<'EOF'
{"name": "issue84 A", "tracks": [{"id": 1, "name": "V1", "kind": "video", "clips": [{"id": 2, "name": "long", "start_ms": 0, "duration_ms": 21000, "source": "demo/opening.mp4", "gain": 1.0}], "volume": 1.0, "pan": 0.0}], "next_id": 3}
EOF
STUDIO_PROJECT_FILE="$OUT/projectA.json" "$BIN" > "$OUT/stdoutA.log" 2> "$OUT/stderrA.log" &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$OUT/stdoutA.log" && break; sleep 0.5; done
wait_drawn
shot "$OUT/01-initial.png"
echo "red pixels initial: $(playhead_red_count "$OUT/01-initial.png")"

# 右端超過（10,000ms）まではオフセットが変わらないこと
sleep 4
if grep -q "^scroll offset:" "$OUT/stdoutA.log"; then echo "FAIL: offset changed before crossing"; else echo "OK: no offset change before crossing"; fi

# ページ送り: scroll offset: 10000 が出る（約10秒時点）
if wait_log "^scroll offset: 10000" 20 "$OUT/stdoutA.log"; then echo "OK: paged to offset 10000"; else echo "FAIL: never paged"; fi
sleep 0.6
shot "$OUT/02-followed.png"
echo "red pixels followed: $(playhead_red_count "$OUT/02-followed.png")"
N_SCROLL=$(grep -c "^scroll offset:" "$OUT/stdoutA.log")
echo "scroll lines so far: $N_SCROLL (expect 1)"
[ "$N_SCROLL" = "1" ] && echo "OK: offset changed only at page timing" || echo "FAIL: unexpected scroll count"

press_f() { # アプリは毎フレーム keys.down をポーリングするので長押しで送る（#31 の知見）
  xdotool keydown f; sleep 0.7; xdotool keyup f; sleep 0.5
}

# F キーで追従オフ → その後は右端超過（20,000ms）とラップ（21,000ms）を
# またいでもオフセットは変わらない
focus_win
press_f
if wait_log "^follow: off" 5 "$OUT/stdoutA.log"; then echo "OK: follow off"; else echo "FAIL: no follow off log"; fi
press_f
if wait_log "^follow: on" 5 "$OUT/stdoutA.log"; then echo "OK: follow on again"; else echo "FAIL: no follow on log"; fi
press_f
sleep 13
N_SCROLL2=$(grep -c "^scroll offset:" "$OUT/stdoutA.log")
echo "scroll lines after F off + 13s: $N_SCROLL2 (expect still 1)"
[ "$N_SCROLL2" = "1" ] && echo "OK: offset frozen while follow off" || echo "FAIL: offset moved while off"
shot "$OUT/03-follow-off.png"
kill $APP 2>/dev/null; wait $APP 2>/dev/null

# ---- Launch B: 終端 11,000ms。ラップアラウンドでオフセット 0 へ ----
cat > "$OUT/projectB.json" <<'EOF'
{"name": "issue84 B", "tracks": [{"id": 1, "name": "V1", "kind": "video", "clips": [{"id": 2, "name": "long", "start_ms": 0, "duration_ms": 11000, "source": "demo/opening.mp4", "gain": 1.0}], "volume": 1.0, "pan": 0.0}], "next_id": 3}
EOF
STUDIO_PROJECT_FILE="$OUT/projectB.json" "$BIN" > "$OUT/stdoutB.log" 2> "$OUT/stderrB.log" &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$OUT/stdoutB.log" && break; sleep 0.5; done
wait_drawn
if wait_log "^scroll offset: 10000" 20 "$OUT/stdoutB.log"; then echo "OK: B paged to 10000"; else echo "FAIL: B never paged"; fi
if wait_log "^scroll offset: 0$" 8 "$OUT/stdoutB.log"; then echo "OK: B offset reset to 0 on wrap"; else echo "FAIL: B no wrap reset"; fi
sleep 0.4
shot "$OUT/04-wrapped.png"
echo "red pixels wrapped: $(playhead_red_count "$OUT/04-wrapped.png")"
kill $APP 2>/dev/null; wait $APP 2>/dev/null

kill $XVFB_PID 2>/dev/null
echo "DRIVE84_DONE"
