#!/bin/bash
# Issue #99 acceptance drive: DAW GUI under Xvfb + lavapipe (:98).
# WAV を XDND でドロップし、配置に加えて Demucs 分離がワーカースレッドで
# 走り、完了後に4ステムのトラックがプロジェクトへ追加されることを確認する。
set -u
export DISPLAY=:98
ROOT=/home/hatch/workspace/repos/rust-studio
XDND=$ROOT/docs/test-reports/evidence/issue-0030/xdnd_source.py
OUT=/home/hatch/demucs-work/gui
mkdir -p "$OUT"
cp /home/hatch/demucs-work/input.wav "$OUT/drop.wav"
# 既存トラックのあるプロジェクトをバイナリ自身に作らせる（形式の正しさ担保）
cp $ROOT/docs/test-reports/evidence/issue-0070/project.seed.json "$OUT/project.json"
cp $ROOT/docs/test-reports/evidence/issue-0070/song.mid "$OUT/song.mid"
"$ROOT/target/debug/daw" --import-midi "$OUT/project.json" "$OUT/song.mid" > "$OUT/seed.log" 2>&1
Xvfb :98 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE="$OUT/project.json" "$ROOT/target/debug/daw" > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
cleanup() { kill $GUI_PID $XVFB_PID 2>/dev/null; }
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
sleep 1
ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :98 -frames:v 1 "$OUT/shot0-before.png"
# WAV をドロップ（配置＋分離が走る）
python3 "$XDND" daw 400 200 "file://$OUT/drop.wav" > "$OUT/xdnd.txt" 2>&1
sleep 1
grep -q "dropped:" "$OUT/gui-stdout.txt" || { echo "FAIL: drop did not import"; cleanup; exit 1; }
grep -q "separation started:" "$OUT/gui-stdout.txt" || { echo "FAIL: separation did not start"; cleanup; exit 1; }
# 分離完了を待つ（実測 ~19 秒。余裕を見て 120 秒）
for i in $(seq 1 120); do grep -q "separated:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 1; done
grep -q "separated:" "$OUT/gui-stdout.txt" || { echo "FAIL: separation did not finish"; tail -5 "$OUT/gui-stdout.txt"; cleanup; exit 1; }
sleep 1
ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :98 -frames:v 1 "$OUT/shot1-separated.png"
cleanup
python3 - "$OUT" <<'PYEOF'
import json, os, sys
out = sys.argv[1]
p = json.load(open(f"{out}/project.json"))
names = [t["name"] for t in p["tracks"]]
print("tracks:", names)
for stem in ("vocals", "drums", "bass", "other"):
    t = next((t for t in p["tracks"] if t["name"] == f"drop / {stem}"), None)
    assert t is not None, f"stem track missing: {stem}"
    assert len(t["clips"]) == 1, t
    c = t["clips"][0]
    assert os.path.isfile(c["source"]), c["source"]
    assert c["duration_ms"] == 2000, c
    assert c["notes"] == [], c
assert "Lead" in names and "Bass" in names, names
print("GUI SEPARATION CHECK OK")
PYEOF
