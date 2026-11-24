#!/bin/bash
# Issue #82 証拠ドライブ: スクリプトフォルダへの配置で一覧に現れる
# こと（有効／無効／エラー／欠落の区別を含む）、一覧が起動時ダンプで
# 観測できること、行のダブルクリックで有効／無効が切り替わること、
# 無効化したスクリプトが適用済みクリップで適用されなくなることを
# 確認する。Xvfb(:96) + lavapipe 前提（実ディスプレイ未検証）。
set -u
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=/tmp/issue82
MED=/tmp/rust-studio-video-editor-media
mkdir -p "$OUT" "$MED/scripts"
export DISPLAY=:96
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=$ROOT/target/debug/video-editor

# --- スクリプト配置: 正常1・構文エラー1・無効化済み1 ---
# アプリの作業フォルダなので、過去のテスト残存を消してから置く。
rm -f "$MED/scripts"/*.lua "$MED/scripts/disabled-scripts.txt"
cat > "$MED/scripts/invert.lua" <<'EOF'
for y = 0, frame.height - 1 do
  for x = 0, frame.width - 1 do
    local r, g, b, a = frame.get(x, y)
    frame.set(x, y, 255 - r, 255 - g, 255 - b, a)
  end
end
EOF
cat > "$MED/scripts/broken.lua" <<'EOF'
return 1 +
EOF
cat > "$MED/scripts/off.lua" <<'EOF'
frame.fill_rect(0, 0, frame.width, frame.height, 0, 128, 255, 255)
EOF
printf 'off.lua\n' > "$MED/scripts/disabled-scripts.txt"
rm -rf "$MED/scriptfx"

# --- プロジェクト: clip1 は invert.lua、clip2 は不在の ghost.lua を参照 ---
cat > "$OUT/project.json" <<'EOF'
{"name": "issue82 drive", "tracks": [
 {"id": 1, "name": "V1", "kind": "video", "clips": [
   {"id": 3, "name": "opening", "start_ms": 0, "duration_ms": 4000, "source": "demo/opening.mp4", "gain": 1.0, "script_effect": {"script": "invert.lua", "params": {}}},
   {"id": 4, "name": "later", "start_ms": 4000, "duration_ms": 4000, "source": "demo/opening.mp4", "gain": 1.0, "script_effect": {"script": "ghost.lua", "params": {}}}
 ], "volume": 1.0, "pan": 0.0}
], "next_id": 6}
EOF

Xvfb :96 -screen 0 1024x768x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE=$OUT/project.json "$BIN" > "$OUT/app.log" 2>&1 &
APP=$!
cleanup() { kill $APP 2>/dev/null; kill $XVFB_PID 2>/dev/null; }
trap cleanup EXIT

for i in $(seq 1 60); do grep -q "^scripts:" "$OUT/app.log" && break; sleep 0.5; done
grep -q "^scripts:" "$OUT/app.log" || { echo "FAIL: no scripts dump"; exit 1; }
for i in $(seq 1 60); do grep -q "^preview:" "$OUT/app.log" && break; sleep 0.5; done
grep -q "^preview:" "$OUT/app.log" || { echo "FAIL: no preview lines"; exit 1; }
WID=$(xdotool search --name "video-editor" | head -1)
xdotool windowfocus "$WID" 2>/dev/null; xdotool windowactivate "$WID" 2>/dev/null
sleep 2

# --- 起動時ダンプ: 一覧と参照状態（欠落の明示を含む） ---
python3 - "$OUT/app.log" <<'EOF'
import json, sys
dump = None
missing_line = None
for line in open(sys.argv[1]):
    if line.startswith("scripts: "):
        dump = json.loads(line[len("scripts: "):])
    if line.startswith("missing scripts:"):
        missing_line = line.strip()
assert dump is not None, "no scripts dump"
scripts = {s["name"]: s for s in dump["scripts"]}
# フォルダには他のテストの残存ファイルもありうるため、期待する
# 3件の存在と状態だけを見る。
for name in ("invert.lua", "broken.lua", "off.lua"):
    assert name in scripts, f"{name} missing from {sorted(scripts)}"
assert scripts["invert.lua"]["enabled"] is True and scripts["invert.lua"]["status"] == "ready"
assert scripts["off.lua"]["enabled"] is False, scripts["off.lua"]
assert isinstance(scripts["broken.lua"]["status"], dict) and "error" in scripts["broken.lua"]["status"], scripts["broken.lua"]
refs = {r["script"]: r["status"] for r in dump["references"]}
assert refs == {"ghost.lua": "missing", "invert.lua": "ready"}, refs
assert missing_line == "missing scripts: ghost.lua", missing_line
print("OK startup dump: entries with enabled/status, ghost.lua missing is explicit")
EOF
[ $? -eq 0 ] || exit 1

shot() { ffmpeg -y -loglevel error -f x11grab -draw_mouse 0 -video_size 960x720 -i :96 -frames:v 1 "$1"; }
shot "$OUT/panel.png"

# パネル行の y 座標: トラック1本のとき scripts_top = 260 + N*20
# （N はメディアビンの件数）。行 i の中心は top + 24 + i*20 + 10。
BIN_LINE=$(grep "^media bin:" "$OUT/app.log" | head -1)
N=$(python3 -c "
line = '''$BIN_LINE'''
names = line[len('media bin:'):].strip()
print(0 if not names else len(names.split(', ')))")
row_y() { echo $(( 294 + N * 20 + $1 * 20 )); }
# 表示行: 0=broken.lua(error) 1=invert.lua(on) 2=off.lua(off) 3=ghost.lua(missing)
dblclick() {
  # 保持式で送る: ソフトウェアレンダリングではフレーム間隔が長く、
  # 短い押下はポーリングに拾われない（#50／#81 のキー入力と同じ知見）。
  # 2回目の押下エッジが1回目から 0.6 秒以内に来るようにする。
  local y=$1
  xdotool mousemove 200 "$y"
  xdotool mousedown 1; sleep 0.2; xdotool mouseup 1; sleep 0.15
  xdotool mousedown 1; sleep 0.2; xdotool mouseup 1; sleep 0.4
}

# --- off.lua 行のダブルクリック: 無効 → 有効 ---
dblclick "$(row_y 2)"
for i in $(seq 1 20); do grep -q "script toggled: off.lua -> enabled" "$OUT/app.log" && break; sleep 0.5; done
grep -q "script toggled: off.lua -> enabled" "$OUT/app.log" || { echo "FAIL: off.lua was not toggled on"; exit 1; }
echo "OK double-click toggled off.lua to enabled"
grep "^scripts:" "$OUT/app.log" | tail -1 | python3 -c "
import json, sys
dump = json.loads(sys.stdin.read()[len('scripts: '):])
scripts = {s['name']: s for s in dump['scripts']}
assert scripts['off.lua']['enabled'] is True, scripts['off.lua']
print('OK dump shows off.lua enabled')
" || { echo "FAIL: dump does not show off.lua enabled"; exit 1; }

# --- invert.lua 行のダブルクリック: 有効 → 無効。
# 再生ヘッドのクリップが参照しているため、適用が止まり
# 「無効化されているため適用できません」が観測できる ---
dblclick "$(row_y 1)"
for i in $(seq 1 20); do grep -q "script toggled: invert.lua -> disabled" "$OUT/app.log" && break; sleep 0.5; done
grep -q "script toggled: invert.lua -> disabled" "$OUT/app.log" || { echo "FAIL: invert.lua was not toggled off"; exit 1; }
echo "OK double-click toggled invert.lua to disabled"
for i in $(seq 1 20); do grep -q "無効化されているため適用できません" "$OUT/app.log" && break; sleep 0.5; done
grep -q "script error: スクリプト invert.lua は無効化されているため適用できません" "$OUT/app.log" || { echo "FAIL: disabled script was still applied or not reported"; exit 1; }
kill -0 $APP 2>/dev/null || { echo "FAIL: app died"; exit 1; }
echo "OK disabled script is not applied; label/log shows it is disabled; app alive"
shot "$OUT/panel-after.png"
echo "DRIVE OK"
