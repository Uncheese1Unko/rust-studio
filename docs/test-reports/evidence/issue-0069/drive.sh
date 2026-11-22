#!/bin/bash
# Issue #69 acceptance drive: DAW GUI under Xvfb + lavapipe (:95).
# ノートクリップをダブルクリックで開き、クリックで配置、
# ドラッグで移動、Delete で削除、Ctrl+Z で Undo、Esc で閉じる。
# 入力プロジェクトはシードから毎回コピーする（アプリが同じ
# ファイルへ保存するため、使い回すと前回の状態が残る）。
# 成否は stdout のダンプと、保存されたプロジェクトファイルで
# 判定する。クリックとキーは長押しで送る（短い押下は低 FPS の
# フレーム間で落ちることを実測で確認したため）。
set -u
export DISPLAY=:95
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=$ROOT/docs/test-reports/evidence/issue-0069
LIVE=/tmp/issue0069-project.json
cp "$OUT/project.seed.json" "$LIVE"
Xvfb :95 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE="$LIVE" "$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :95 -frames:v 1 "$OUT/$1"; }
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
focus_win() {
  local wid
  wid=$(xdotool search --name "^daw$" 2>/dev/null | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  xdotool mousemove 60 48; sleep 0.2; xdotool mousedown 1; sleep 0.3; xdotool mouseup 1; sleep 0.4
}
click_at() { xdotool mousemove "$1" "$2"; sleep 0.25; xdotool mousedown 1; sleep 0.3; xdotool mouseup 1; sleep 0.4; }
double_click_at() {
  xdotool mousemove "$1" "$2"; sleep 0.2
  xdotool mousedown 1; sleep 0.25; xdotool mouseup 1; sleep 0.05
  xdotool mousedown 1; sleep 0.25; xdotool mouseup 1; sleep 0.4
}
wait_marker() {
  local marker="$1" i
  for i in $(seq 1 30); do grep -q "$marker" "$OUT/gui-stdout.txt" && return 0; sleep 0.4; done
  echo "FAIL: marker not seen: $marker"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1
}
notes_json() {
  python3 - "$LIVE" <<'PYEOF2'
import json, sys
try:
    p = json.load(open(sys.argv[1]))
    print(json.dumps(p["tracks"][0]["clips"][0]["notes"]))
except Exception:
    print("[]")
PYEOF2
}
focus_win
shot shot1-timeline.png
# 1) クリップ（x 120..456, レーン y 24..72）のダブルクリックで開く
for try in 1 2 3 4 5; do
  grep -q "piano roll opened: clip 2" "$OUT/gui-stdout.txt" && break
  focus_win
  double_click_at 300 48
  sleep 0.4
done
grep -q "piano roll opened: clip 2" "$OUT/gui-stdout.txt" || { echo "FAIL: roll never opened"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
sleep 0.8
shot shot2-roll-open.png
# 2) 空きグリッドをクリックして配置（pitch 62, 約1000ms）
for try in 1 2 3 4 5; do
  [ "$(notes_json)" != "[]" ] && break
  click_at 275 252
  sleep 0.4
done
[ "$(notes_json)" != "[]" ] || { echo "FAIL: note never placed"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
wait_marker "after note add:"
sleep 0.5
shot shot3-note-placed.png
# 3) ノートをドラッグで移動（pitch 65, 約2000ms へ）。押下が
# 落ちたらプロジェクトが変わらないので、同じドラッグをやり直す
# （移動済みなら目標状態の判定で止まる）。
drag_note() {
  # ドラッグの始点と終点は、実際のノート位置（LIVE）から
  # レイアウト式で計算する（座標の当て推量をしない）。
  local coords
  coords=$(python3 - "$LIVE" <<'PYEOF2'
import json, sys
p = json.load(open(sys.argv[1]))
n = p["tracks"][0]["clips"][0]["notes"][0]
GRID_X, GRID_Y, GRID_W = 46.0, 50.0, 914.0
ROW_H = (540.0 - GRID_Y - 8.0) / 25.0
TOP = 72
def x_of(ms): return GRID_X + ms / 4000.0 * GRID_W
def y_of(pitch): return GRID_Y + (TOP - pitch) * ROW_H + ROW_H / 2.0
sx = x_of(n["start_ms"] + 100)
sy = y_of(n["pitch"])
grab_ms = round((sx - GRID_X) / GRID_W * 4000)
target_pointer_ms = grab_ms + (2000 - n["start_ms"])
ex = x_of(target_pointer_ms)
ey = y_of(65)
print(f"{sx:.0f} {sy:.0f} {ex:.0f} {ey:.0f}")
PYEOF2
)
  local sx sy ex ey
  read -r sx sy ex ey <<< "$coords"
  echo "drag: ($sx,$sy) -> ($ex,$ey)"
  xdotool mousemove "$sx" "$sy"; sleep 0.5
  xdotool mousedown 1; sleep 0.8
  xdotool mousemove $(( (sx + ex) / 2 )) $(( (sy + ey) / 2 )); sleep 0.4
  xdotool mousemove "$ex" "$ey"; sleep 0.5
  xdotool mouseup 1; sleep 0.8
}
moved() {
  python3 - "$LIVE" <<'PYEOF2'
import json, sys
try:
    p = json.load(open(sys.argv[1]))
    notes = p["tracks"][0]["clips"][0]["notes"]
    ok = len(notes) == 1 and notes[0]["pitch"] == 65 and abs(notes[0]["start_ms"] - 2000) <= 6
except Exception:
    ok = False
sys.exit(0 if ok else 1)
PYEOF2
}
for try in 1 2 3; do
  moved && break
  drag_note
done
moved || { echo "FAIL: note never moved (state: $(notes_json))"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
wait_marker "after note move:"
shot shot4-note-moved.png
# 4) Delete で削除（ドラッグ後もノートは選択されたまま）
for try in 1 2 3; do
  grep -q "after note remove:" "$OUT/gui-stdout.txt" && break
  xdotool keydown Delete; sleep 0.35; xdotool keyup Delete; sleep 0.6
done
wait_marker "after note remove:"
# 5) Ctrl+Z で Undo（ノートが戻る）
for try in 1 2 3; do
  grep -q "after undo:" "$OUT/gui-stdout.txt" && break
  xdotool keydown ctrl; sleep 0.2; xdotool keydown z; sleep 0.4; xdotool keyup z; sleep 0.2; xdotool keyup ctrl; sleep 0.6
done
wait_marker "after undo:"
# 6) Esc で閉じる
for try in 1 2 3 4 5; do
  grep -q "piano roll: closed" "$OUT/gui-stdout.txt" && break
  xdotool keydown Escape; sleep 0.35; xdotool keyup Escape; sleep 0.6
done
wait_marker "piano roll: closed"
shot shot5-timeline-again.png
cp "$LIVE" "$OUT/project-final.json"
kill "$GUI_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
text = open(f"{out}/gui-stdout.txt").read()
def last_dump(marker):
    idx = text.rfind(marker)
    assert idx >= 0, f"no dump for {marker}"
    dump, _ = json.JSONDecoder().raw_decode(text[idx + len(marker):])
    return dump
notes = last_dump("after note add: ")["tracks"][0]["clips"][0]["notes"]
assert len(notes) == 1, notes
n = notes[0]
assert n["pitch"] == 62 and abs(n["start_ms"] - 1000) <= 6 and n["duration_ms"] == 500, n
print(f"place OK: pitch 62 at {n['start_ms']}ms, 500ms")
notes = last_dump("after note move: ")["tracks"][0]["clips"][0]["notes"]
assert len(notes) == 1, notes
n = notes[0]
assert n["pitch"] == 65 and abs(n["start_ms"] - 2000) <= 6, n
print(f"move OK: pitch 65 at {n['start_ms']}ms")
notes = last_dump("after note remove: ")["tracks"][0]["clips"][0]["notes"]
assert notes == [], notes
print("remove OK: no notes")
notes = last_dump("after undo: ")["tracks"][0]["clips"][0]["notes"]
assert len(notes) == 1 and notes[0]["pitch"] == 65, notes
print("undo OK: the moved note is back")
print("DRIVE OK")
PYEOF
