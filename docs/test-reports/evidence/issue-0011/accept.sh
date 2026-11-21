#!/bin/bash
# Issue #11 acceptance: MCP edits -> shared project file -> GUI reflects them.
# (再現用スクリプト。実行すると OUT を作り直す)
set -u
export DISPLAY=:99
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=/tmp/ve11
rm -rf "$OUT"; mkdir -p "$OUT"
PROJ="$OUT/project.json"
FIFO="$OUT/mcp-in"
mkfifo "$FIFO"

Xvfb :99 -screen 0 1024x600x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1

# MCP server with project-file backing; stdin via FIFO (kept open by fd 3)
"$ROOT/target/debug/studio-mcp" --project-file "$PROJ" < "$FIFO" > "$OUT/mcp-out.jsonl" 2> "$OUT/mcp-err.log" &
MCP_PID=$!
exec 3> "$FIFO"

send() { echo "$1" >&3; sleep 0.4; }

send '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}'
send '{"jsonrpc":"2.0","method":"notifications/initialized"}'
send '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add_track","arguments":{"name":"V1","kind":"video"}}}'
send '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"add_clip","arguments":{"track_id":1,"name":"opening","start_ms":0,"duration_ms":2000,"source":"demo/opening.mp4"}}}'
send '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"add_clip","arguments":{"track_id":1,"name":"interview","start_ms":2500,"duration_ms":4000,"source":"demo/interview.mp4"}}}'
send '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"get_project","arguments":{}}}'
sleep 0.5

# GUI in project-file mode
STUDIO_PROJECT_FILE="$PROJ" "$ROOT/target/debug/video-editor" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 40); do grep -q "preview textures loaded" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 1.5
ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$OUT/shot1-mcp-state.png"

# MCP move: opening 0 -> 1000
send '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"move_clip","arguments":{"clip_id":2,"new_start_ms":1000}}}'
send '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"get_project","arguments":{}}}'
sleep 2.5
ffmpeg -y -loglevel error -f x11grab -video_size 1024x600 -i :99 -frames:v 1 "$OUT/shot2-after-move.png"

kill "$GUI_PID" 2>/dev/null
exec 3>&-
kill "$MCP_PID" 2>/dev/null
kill "$XVFB_PID" 2>/dev/null
echo "done"
