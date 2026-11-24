#!/bin/bash
# Issue #86 証拠ドライブ: DAW の再生ヘッド表示追従（ページ送り）、
# F キーでのオン／オフ、終端ラップとループ折り返しでのオフセット 0
# リセットを実アプリで確認する。Xvfb(:97) + lavapipe 前提
# （実ディスプレイ未検証）。
set -u
ROOT=/home/hatch/workspace/repos/rust-studio-lane-b
OUT=/tmp/issue86
mkdir -p "$OUT"
export DISPLAY=:97
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=$ROOT/target/debug/daw
cd "$ROOT"

# 駆動用の音源（22秒の 220Hz サイン波）
python3 - <<'PYEOF'
import wave, struct, math
w = wave.open('/tmp/issue86/tone.wav', 'wb')
w.setnchannels(1); w.setsampwidth(2); w.setframerate(44100)
frames = b''.join(struct.pack('<h', int(8000*math.sin(2*math.pi*220*i/44100))) for i in range(44100))
w.writeframes(frames); w.close()
PYEOF

Xvfb :97 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1

shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :97 -frames:v 1 "$1"; }
focus_win() {
  local wid
  wid=$(xdotool search --name "daw" 2>/dev/null | head -1)
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
# 再生ヘッド（theme::TEXT = (33,24,18) の縦線）が画面内にあるか。
# 縦1列で TEXT 色に近い画素が最も多い列の画素数を返す。
playhead_line_max() {
  python3 - "$1" <<'PYEOF'
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert('RGB')
best = 0
for x in range(120, 960):
    n = 0
    for y in range(0, 540):
        r, g, b = im.getpixel((x, y))
        if abs(r-33) < 14 and abs(g-24) < 14 and abs(b-18) < 14:
            n += 1
    best = max(best, n)
print(best)
PYEOF
}
wait_log() { # $1=pattern $2=timeout_s $3=logfile
  for i in $(seq 1 $(( $2 * 4 ))); do
    grep -q "$1" "$3" && return 0
    sleep 0.25
  done
  return 1
}
press_f() { # アプリは毎フレーム keys.down をポーリングするので長押しで送る（#31 の知見）
  xdotool keydown f; sleep 0.7; xdotool keyup f; sleep 0.5
}
mkproject() { # $1=file $2=end_ms
  cat > "$1" <<EOF
{"name": "issue86", "tracks": [{"id": 1, "name": "A1", "kind": "audio", "clips": [{"id": 2, "name": "long", "start_ms": 0, "duration_ms": $2, "source": "/tmp/issue86/tone.wav", "gain": 1.0}], "volume": 1.0, "pan": 0.0}], "next_id": 3}
EOF
}

# ---- Launch A: 終端 21,000ms。ページ送りと F オフを確認 ----
mkproject "$OUT/projectA.json" 21000
STUDIO_PROJECT_FILE="$OUT/projectA.json" "$BIN" > "$OUT/stdoutA.log" 2> "$OUT/stderrA.log" &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$OUT/stdoutA.log" && break; sleep 0.5; done
wait_drawn
shot "$OUT/01-initial.png"

# 右端超過（10,000ms）まではオフセットが変わらないこと
sleep 4
if grep -q "^scroll offset:" "$OUT/stdoutA.log"; then echo "FAIL: offset changed before crossing"; else echo "OK: no offset change before crossing"; fi

# ページ送り: scroll offset: 10000 が出る（約10秒時点）
if wait_log "^scroll offset: 10000" 20 "$OUT/stdoutA.log"; then echo "OK: paged to offset 10000"; else echo "FAIL: never paged"; fi
sleep 0.6
shot "$OUT/02-followed.png"
echo "playhead line max followed: $(playhead_line_max "$OUT/02-followed.png")"
N_SCROLL=$(grep -c "^scroll offset:" "$OUT/stdoutA.log")
echo "scroll lines so far: $N_SCROLL (expect 1)"
[ "$N_SCROLL" = "1" ] && echo "OK: offset changed only at page timing" || echo "FAIL: unexpected scroll count"

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
mkproject "$OUT/projectB.json" 11000
STUDIO_PROJECT_FILE="$OUT/projectB.json" "$BIN" > "$OUT/stdoutB.log" 2> "$OUT/stderrB.log" &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$OUT/stdoutB.log" && break; sleep 0.5; done
wait_drawn
if wait_log "^scroll offset: 10000" 20 "$OUT/stdoutB.log"; then echo "OK: B paged to 10000"; else echo "FAIL: B never paged"; fi
if wait_log "^scroll offset: 0$" 8 "$OUT/stdoutB.log"; then echo "OK: B offset reset to 0 on wrap"; else echo "FAIL: B no wrap reset"; fi
sleep 0.4
shot "$OUT/04-wrapped.png"
kill $APP 2>/dev/null; wait $APP 2>/dev/null

# ---- Launch C: 終端 21,000ms。ループ折り返しでオフセット 0 へ ----
mkproject "$OUT/projectC.json" 21000
STUDIO_PROJECT_FILE="$OUT/projectC.json" "$BIN" > "$OUT/stdoutC.log" 2> "$OUT/stderrC.log" &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$OUT/stdoutC.log" && break; sleep 0.5; done
wait_drawn
if wait_log "^scroll offset: 10000" 20 "$OUT/stdoutC.log"; then echo "OK: C paged to 10000"; else echo "FAIL: C never paged"; fi
# 表示範囲 10,000..20,000 のルーラー上で 12,000→15,000ms をドラッグ
# x = 120 + (ms - 10000) * 0.084 → 12,000: 288, 15,000: 540。y=12 はルーラー帯。
focus_win
xdotool mousemove 288 12; sleep 0.3
xdotool mousedown 1; sleep 0.3
xdotool mousemove 540 12; sleep 0.5
xdotool mouseup 1; sleep 0.5
if wait_log "^loop range:" 5 "$OUT/stdoutC.log"; then echo "OK: loop set: $(grep '^loop range:' "$OUT/stdoutC.log" | tail -1)"; else echo "FAIL: loop not set"; fi
# ループ終端 15,000 で折り返しても、折り返し先（12,000）を含む
# ページ（オフセット 10,000）に留まるため `scroll offset: 0` は
# 出ない（レビュー対応後の振る舞い）。再生ヘッドは見えたまま。
sleep 6
LOOP_LINE=$(grep -n "^loop range:" "$OUT/stdoutC.log" | tail -1 | cut -d: -f1)
if tail -n +"$LOOP_LINE" "$OUT/stdoutC.log" | grep -q "^scroll offset: 0$"; then echo "FAIL: C offset reset to 0 on loop wrap"; else echo "OK: C offset stayed on loop-start page at loop wrap"; fi
shot "$OUT/05-loop-wrapped.png"
echo "playhead line max loop-wrapped: $(playhead_line_max "$OUT/05-loop-wrapped.png")"
kill $APP 2>/dev/null; wait $APP 2>/dev/null

kill $XVFB_PID 2>/dev/null
echo "DRIVE86_DONE"
