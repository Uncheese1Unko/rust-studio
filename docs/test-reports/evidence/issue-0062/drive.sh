#!/bin/bash
# Issue #62 acceptance drive: DAW GUI under Xvfb + lavapipe (:95).
# offset_ms 1500 のクリップを開き、Ctrl+D（複製）→ Ctrl+C（コピー）→
# Ctrl+V（貼り付け）を通し、ダンプでコピー側の offset_ms が 1500 に
# 一致することを確認する。
#
# 駆動上の注意（この環境で実測した挙動）:
# - 起動直後のアプリはフォーカスを得るまで描画も時計も進まない。
#   elements: の出力だけでは不十分で、実際に描画が始まったことを
#   スクリーンショットの非黒ピクセルで確認してからキーを送る。
# - 再生ヘッドは実時間で進む。クリップは 0〜8500ms に置き、複製後は
#   終端が 17000ms になるため、コピーの対象は元か複製のどちらかに
#   必ず収まる（どちらも offset_ms 1500）。
# - キーは #31/#33 の方式（マーカーが出るまで再フォーカスしながら
#   最大5回リトライ）。Xvfb 上では単発のキー送信が取りこぼされる
#   ことがある。
set -u
export DISPLAY=:95
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=${1:-/tmp/daw62}
mkdir -p "$OUT" /tmp/rust-studio-daw-media
python3 - "$OUT" <<'PYEOF'
import json, math, os, struct, sys, wave
out = sys.argv[1]
tone = "/tmp/rust-studio-daw-media/tone440.wav"
if not os.path.exists(tone):
    w = wave.open(tone, "wb")
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(44100)
    w.writeframes(b"".join(
        struct.pack("<h", int(0.4 * 32767 * math.sin(2 * math.pi * 440 * i / 44100)))
        for i in range(44100 * 10)))
    w.close()
project = {
  "name": "offset duplicate drive",
  "tracks": [{
    "id": 1, "name": "A1", "kind": "audio",
    "clips": [{
      "id": 2, "name": "phrase", "start_ms": 0, "duration_ms": 8500,
      "source": tone,
      "offset_ms": 1500, "gain": 0.5, "cutout": False,
      "pitch_semitones": 2.0, "color": None
    }],
    "volume": 1.0, "pan": 0.0, "color": None
  }],
  "next_id": 3
}
open(f"{out}/project.json", "w").write(json.dumps(project))
PYEOF
Xvfb :95 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE="$OUT/project.json" "$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :95 -frames:v 1 "$OUT/$1"; }
# 実際に描画が始まるまで待つ（黒画面のうちはアプリが起きていない）
for i in $(seq 1 40); do
  shot probe.png
  if python3 -c "
from PIL import Image
import sys
im = Image.open('$OUT/probe.png').convert('RGB')
sys.exit(0 if im.getpixel((480, 270)) != (0, 0, 0) else 1)
"; then break; fi
  sleep 0.5
done
focus_header() {
  local wid
  wid=$(xdotool search --name "^daw$" 2>/dev/null | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  xdotool mousemove 60 48; sleep 0.2
  xdotool mousedown 1; sleep 0.4; xdotool mouseup 1; sleep 0.5
}
press_ctrl() { # key
  xdotool keydown ctrl; sleep 0.2
  xdotool keydown "$1"; sleep 0.5
  xdotool keyup "$1"; sleep 0.2
  xdotool keyup ctrl; sleep 0.3
}
send_until() { # marker key
  local marker="$1" key="$2" try
  for try in 1 2 3 4 5; do
    if grep -q "$marker" "$OUT/gui-stdout.txt"; then return 0; fi
    focus_header
    press_ctrl "$key"
    sleep 0.6
  done
  grep -q "$marker" "$OUT/gui-stdout.txt"
}
focus_header
sleep 1
shot shot1-initial.png
send_until "after duplicate" d || { echo "FAIL: duplicate never fired"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
shot shot2-duplicated.png
# 複製の適用後にポインタ状態（ドラッグ等）が残ると、後続の編集キーが
# handle_edit_keys の早期 return で捨てられる（実測）。中立位置の
# クリックと Escape で状態を落としてから次のキーに進む。
reset_pointer_state() {
  xdotool mousemove 480 400; sleep 0.2
  xdotool mousedown 1; sleep 0.4; xdotool mouseup 1; sleep 0.8
  xdotool key Escape; sleep 0.8
}
reset_pointer_state
send_until "copied clip" c || { echo "FAIL: copy never fired"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
reset_pointer_state
send_until "after paste" v || { echo "FAIL: paste never fired"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
shot shot3-pasted.png
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import sys, json
from PIL import Image
out = sys.argv[1]
text = open(f"{out}/gui-stdout.txt").read()
def last_dump(marker):
    idx = text.rfind(marker)
    assert idx >= 0, f"no {marker} dump"
    dump, _ = json.JSONDecoder().raw_decode(text[idx + len(marker):])
    return dump
dup = last_dump("after duplicate: ")
clips = sorted(dup["tracks"][0]["clips"], key=lambda c: c["start_ms"])
assert len(clips) == 2, f"duplicate: expected 2 clips, got {len(clips)}"
first, second = clips
assert (first["start_ms"], first["offset_ms"]) == (0, 1500), first
assert second["start_ms"] == 8500, second
assert second["offset_ms"] == 1500, f"duplicated offset must be 1500: {second}"
for key in ["gain", "pitch_semitones", "name", "source", "duration_ms"]:
    assert second[key] == first[key], f"param {key} differs: {first} vs {second}"
print("duplicate OK: second clip at 8500 with offset_ms 1500 copied")
paste = last_dump("after paste: ")
clips = paste["tracks"][0]["clips"]
assert len(clips) == 3, f"paste: expected 3 clips, got {len(clips)}"
pasted = max(clips, key=lambda c: c["id"])
assert pasted["offset_ms"] == 1500, f"pasted offset must be 1500: {pasted}"
assert pasted["gain"] == 0.5 and pasted["pitch_semitones"] == 2.0, pasted
assert pasted["duration_ms"] == 8500, pasted
print(f"paste OK: pasted clip id={pasted['id']} at {pasted['start_ms']}ms with offset_ms 1500")
PANEL = (255, 253, 249)
FACES = {(255, 46, 136), (255, 199, 222)}
def px(path, x, y=60):
    return Image.open(path).convert("RGB").getpixel((x, y))
# ピクセル検証は固定座標の shot1 vs shot3 で行う（ビューの表示範囲は
# プロジェクト長に追従して変わるため、shot2 との固定座標比較は
# 意味を持たない）。x=900 は初期状態では空のトラック行（地色）で、
# 貼り付け後は再生中クリップの面になる。
b = px(f"{out}/shot1-initial.png", 900)
a = px(f"{out}/shot3-pasted.png", 900)
print("shot1 @x900:", b, "| shot3 @x900:", a)
assert b == PANEL and a in FACES
print("PIXEL CHECK OK")
PYEOF
