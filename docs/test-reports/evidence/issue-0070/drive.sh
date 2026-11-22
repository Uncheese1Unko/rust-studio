#!/usr/bin/env bash
# Issue #70 の駆動証拠: CLI 取り込み（正常・異常）、MCP 取り込み、
# GUI で取り込み結果を開いてピアノロールでノートを見る。
# 使い方: bash drive.sh [DAW_BINARY] [MCP_BINARY]
# 前提: xdotool / xwd / ImageMagick (convert) / python3、DISPLAY は
# スクリプトが :96 に Xvfb を立てる。プロジェクトファイルは
# 実行のたびにシードから作り直す（アプリが同じファイルへ保存する）。
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
DAW="${1:-$HERE/../../../../target/debug/daw}"
MCP="${2:-$HERE/../../../../target/debug/studio-mcp}"
export DISPLAY=:96
W=1100; H=700

cp "$HERE/project.seed.json" /tmp/issue0070-project.json
echo "== CLI: --import-midi =="
"$DAW" --import-midi /tmp/issue0070-project.json "$HERE/song.mid" \
  | grep "midi imported" || { echo "FAIL: cli import"; exit 1; }
python3 - << 'PYEOF' || exit 1
import json
p = json.load(open("/tmp/issue0070-project.json"))
assert p["bpm"] == 150.0, p["bpm"]
names = [t["name"] for t in p["tracks"]]
assert names == ["Lead", "Bass"], names
lead = p["tracks"][0]["clips"][0]
assert [n["pitch"] for n in lead["notes"]] == [69, 72, 76], lead["notes"]
assert lead["notes"][0]["duration_ms"] == 400  # 480 tick @150BPM
bass = p["tracks"][1]["clips"][0]
assert [n["pitch"] for n in bass["notes"]] == [45, 41]
print("CLI import assertions OK")
PYEOF

echo "== CLI: garbage file is an error, project unchanged =="
printf 'garbage' > /tmp/issue0070-bad.mid
cp "$HERE/project.seed.json" /tmp/issue0070-bad.json
if "$DAW" --import-midi /tmp/issue0070-bad.json /tmp/issue0070-bad.mid 2>/dev/null; then
  echo "FAIL: garbage accepted"; exit 1
fi
cmp -s /tmp/issue0070-bad.json "$HERE/project.seed.json" \
  && echo "error case OK (exit 1, file unchanged)" || { echo "FAIL: file changed"; exit 1; }

echo "== MCP: import_midi tool =="
python3 - "$MCP" "$HERE/song.mid" << 'PYEOF' || exit 1
import json, subprocess, sys
proc = subprocess.Popen([sys.argv[1]], stdin=subprocess.PIPE,
                        stdout=subprocess.PIPE, text=True)
def send(obj):
    proc.stdin.write(json.dumps(obj) + "\n"); proc.stdin.flush()
    return json.loads(proc.stdout.readline())
send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                 "clientInfo": {"name": "drive", "version": "0"}}})
# 通知には応答が無いので読み取りをしない。
proc.stdin.write(json.dumps({"jsonrpc": "2.0",
    "method": "notifications/initialized", "params": {}}) + "\n")
proc.stdin.flush()
r = send({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
          "params": {"name": "import_midi", "arguments": {"path": sys.argv[2]}}})
body = json.loads(r["result"]["content"][0]["text"])
assert body["ok"] and body["tracks_added"] == 2 and body["notes_added"] == 5, body
assert body["bpm"] == 150.0, body
r = send({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
          "params": {"name": "get_project", "arguments": {}}})
proj = json.loads(r["result"]["content"][0]["text"])
assert [t["name"] for t in proj["tracks"]] == ["Lead", "Bass"]
proc.stdin.close(); proc.terminate()
print("MCP import assertions OK")
PYEOF

echo "== GUI: open imported project, screenshot timeline + piano roll =="
Xvfb :96 -screen 0 960x540x24 > /tmp/issue0070-xvfb.log 2>&1 &
XVFB_PID=$!
sleep 2
STUDIO_PROJECT_FILE=/tmp/issue0070-project.json "$DAW" \
  > "$HERE/gui-stdout.txt" 2> "$HERE/gui-stderr.txt" &
APP_PID=$!
for i in $(seq 1 60); do
  grep -q "elements:" "$HERE/gui-stdout.txt" 2>/dev/null && break; sleep 0.5
done
grep -q "project file mode: /tmp/issue0070-project.json" "$HERE/gui-stdout.txt" \
  || { echo "FAIL: project file not loaded"; kill -9 $APP_PID $XVFB_PID 2>/dev/null; exit 1; }
snap() {
  ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 \
    -i :96 -frames:v 1 "$1"
  echo "saved $1"
}
focus_win() {
  local wid
  wid=$(xdotool search --name "^daw$" 2>/dev/null | head -1)
  [ -n "$wid" ] && xdotool windowfocus "$wid" 2>/dev/null
  xdotool mousemove 60 48; sleep 0.2; xdotool mousedown 1; sleep 0.3
  xdotool mouseup 1; sleep 0.4
}
focus_win
snap "$HERE/shot1-timeline.png"
# Lead クリップ（1行目、スクリーンショット実測で x=120..245、
# y=40..75）をダブルクリックしてピアノロールを開く
# （長押しクリックで送る）。
opened=0
for i in 1 2 3 4 5; do
  xdotool mousemove 182 57; sleep 0.2
  xdotool mousedown 1; sleep 0.25; xdotool mouseup 1; sleep 0.05
  xdotool mousedown 1; sleep 0.25; xdotool mouseup 1; sleep 0.6
  grep -q 'piano roll opened:' "$HERE/gui-stdout.txt" && { opened=1; break; }
done
[ "$opened" = 1 ] \
  || { echo "FAIL: piano roll did not open"; kill -9 $APP_PID $XVFB_PID 2>/dev/null; exit 1; }
sleep 1
snap "$HERE/shot2-piano-roll.png"
xdotool keydown Escape; sleep 0.6; xdotool keyup Escape; sleep 0.5
kill $APP_PID 2>/dev/null; kill $XVFB_PID 2>/dev/null
echo "DRIVE OK"
