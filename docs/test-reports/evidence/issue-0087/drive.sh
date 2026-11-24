#!/bin/bash
# Issue #87 証拠ドライブ: DAW の見渡し手段（表示幅比例ホイール、
# 横ホイール／Shift+ホイール、スクロールバーのドラッグ、中ボタン
# パン、Ctrl+ホイールズームとクリップドラッグの回帰）を実アプリで
# 確認する。Xvfb(:97) + lavapipe 前提（実ディスプレイ未検証）。
#
# 環境の知見: 本 X11 スタックでは xdotool の click 1回でホイール
# イベントが2ノッチ届く（press/release 両方）。そのため期待値は
# 「1ノッチ = 1,000ms」を前提に、click 回数×2ノッチで計算する。
set -u
ROOT=/home/hatch/workspace/repos/rust-studio-lane-b
OUT=/tmp/issue87
mkdir -p "$OUT"
export DISPLAY=:97
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=$ROOT/target/debug/daw
cd "$ROOT"

# 駆動用の音源（1秒の 220Hz サイン波。#86 の知見: 長い音源だと
# F キーのノートオンのピッチシフトが update をブロックする）
python3 - <<'PYEOF'
import wave, struct, math
w = wave.open('/tmp/issue87/tone.wav', 'wb')
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
wait_log() { # $1=pattern $2=timeout_s $3=logfile
  for i in $(seq 1 $(( $2 * 4 ))); do
    grep -q "$1" "$3" && return 0
    sleep 0.25
  done
  return 1
}
last_offset() { grep "^scroll offset:" "$LOG" | tail -1 | awk '{print $3}'; }
press_f() { # アプリは毎フレーム keys.down をポーリングするので長押しで送る（#31 の知見）
  xdotool keydown f; sleep 0.7; xdotool keyup f; sleep 0.5
}
# 帯（y=78）のピクセル種別: thumb=TEXT_SUB(168,152,128) / band=LINE(224,210,194)
band_kind() { # $1=file $2=x
  python3 - "$1" "$2" <<'PYEOF'
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert('RGB')
r, g, b = im.getpixel((int(sys.argv[2]), 78))
if abs(r-168) < 25 and abs(g-152) < 25 and abs(b-128) < 25:
    print("thumb")
elif abs(r-224) < 25 and abs(g-210) < 25 and abs(b-194) < 25:
    print("band")
else:
    print(f"other({r},{g},{b})")
PYEOF
}
LOG="$OUT/stdout.log"

cat > "$OUT/project.json" <<'EOF'
{"name": "issue87", "tracks": [{"id": 1, "name": "A1", "kind": "audio", "clips": [{"id": 2, "name": "long", "start_ms": 0, "duration_ms": 21000, "source": "/tmp/issue87/tone.wav", "gain": 1.0}], "volume": 1.0, "pan": 0.0}], "next_id": 3}
EOF
STUDIO_PROJECT_FILE="$OUT/project.json" "$BIN" > "$LOG" 2> "$OUT/stderr.log" &
APP=$!
for i in $(seq 1 60); do grep -q "^elements:" "$LOG" && break; sleep 0.5; done
wait_drawn
focus_win
shot "$OUT/01-initial.png"
echo "band x=200 (expect thumb): $(band_kind "$OUT/01-initial.png" 200)"
echo "band x=900 (expect band): $(band_kind "$OUT/01-initial.png" 900)"

# 再生追従を切って、以降のオフセット変化が入力だけに依るようにする
press_f
if wait_log "^follow: off" 5 "$LOG"; then echo "OK: follow off"; else echo "FAIL: no follow off log"; fi

# ホイール: 1ノッチ = 表示幅 10,000 の 1/10 = 1,000ms。
# click 1回 = 2ノッチなので、下2回で +4,000、上1回で -2,000。
xdotool mousemove 600 48; sleep 0.2
xdotool click 5; sleep 0.5; xdotool click 5; sleep 0.5
OFF=$(last_offset); echo "offset after wheel down x2 clicks: $OFF (expect 4000)"
[ "$OFF" = "4000" ] && echo "OK: wheel proportional" || echo "FAIL: wheel proportional"
xdotool click 4; sleep 0.5
OFF=$(last_offset); echo "offset after wheel up x1 click: $OFF (expect 2000)"
[ "$OFF" = "2000" ] && echo "OK: wheel up" || echo "FAIL: wheel up"
# 横ホイール: ボタン6 = x+（進む）、ボタン7 = x-（戻る）
xdotool click 6; sleep 0.5
OFF=$(last_offset); echo "offset after button6: $OFF (expect 4000)"
[ "$OFF" = "4000" ] && echo "OK: h-wheel right" || echo "FAIL: h-wheel right"
xdotool click 7; sleep 0.5
OFF=$(last_offset); echo "offset after button7: $OFF (expect 2000)"
[ "$OFF" = "2000" ] && echo "OK: h-wheel left" || echo "FAIL: h-wheel left"
# Shift+ホイール下: x か y どちらで届いても進む
xdotool keydown shift; sleep 0.2; xdotool click 5; sleep 0.3; xdotool keyup shift; sleep 0.5
OFF=$(last_offset); echo "offset after shift+wheel: $OFF (expect 4000)"
[ "$OFF" = "4000" ] && echo "OK: shift+wheel" || echo "FAIL: shift+wheel"

# 中ボタンパン: (700,48) から左へ 150px ずつ2ステップ。
# 1ステップ = 150px / 0.084px-per-ms = 約1,786ms。
xdotool mousemove 700 48; sleep 0.2; xdotool mousedown 2; sleep 0.3
xdotool mousemove 550 48; sleep 0.3; xdotool mousemove 400 48; sleep 0.3
xdotool mouseup 2; sleep 0.5
OFF=$(last_offset); echo "offset after pan: $OFF (expect ~7572)"
[ "${OFF:-0}" -gt 7000 ] && [ "${OFF:-0}" -lt 8000 ] && echo "OK: middle pan" || echo "FAIL: middle pan"

# スクロールバーのドラッグ: この時点のつまみは x=423..823
# （オフセット 7,572）。帯内のつまみの外（x=320）を押すと
# 「つまみ中心で掴む」ため中心がポインタへ寄り、オフセットは
# 0 になる。そのまま右端（940）まで引くと上限 11,000 で止まる。
xdotool mousemove 320 78; sleep 0.2; xdotool mousedown 1; sleep 0.3
xdotool mousemove 630 78; sleep 0.3; xdotool mousemove 940 78; sleep 0.3
xdotool mouseup 1; sleep 0.5
if wait_log "^scrollbar drag end: offset 11000" 5 "$LOG"; then echo "OK: scrollbar drag clamped at 11000"; else echo "FAIL: scrollbar drag"; fi
shot "$OUT/02-dragged-end.png"
echo "band x=900 (expect thumb): $(band_kind "$OUT/02-dragged-end.png" 900)"
echo "band x=200 (expect band): $(band_kind "$OUT/02-dragged-end.png" 200)"

# 回帰: Ctrl+ホイールでズームできる
xdotool mousemove 500 48; sleep 0.2
xdotool keydown ctrl; sleep 0.2; xdotool click 4; sleep 0.3; xdotool keyup ctrl; sleep 0.5
if grep -q "^zoom:" "$LOG"; then echo "OK: ctrl+wheel zoom"; else echo "FAIL: ctrl+wheel zoom"; fi
# 回帰: クリップをドラッグできる（オフセット上限の表示で、クリップは
# 画面左寄りに見えている。見えている部分を掴んで右へ少し動かす）
xdotool mousemove 200 48; sleep 0.2; xdotool mousedown 1; sleep 0.45
xdotool mousemove 260 48; sleep 0.4; xdotool mouseup 1; sleep 0.5
if grep -q "^after drag:" "$LOG"; then echo "OK: clip drag"; else echo "FAIL: clip drag"; fi
shot "$OUT/03-after-checks.png"

kill $APP 2>/dev/null; wait $APP 2>/dev/null
kill $XVFB_PID 2>/dev/null
echo "drive done"
