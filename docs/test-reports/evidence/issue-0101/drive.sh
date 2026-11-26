#!/bin/bash
# Issue #101 acceptance drive: DAW GUI under Xvfb + lavapipe (:98).
# メロディ WAV を XDND でドロップし、分離→抽出→音色登録までが
# GUI で観測できること（タイムラインのノートクリップ、tones パネル、
# 再生の進行）を確認する。
set -u
export DISPLAY=:98
ROOT=/home/hatch/workspace/repos/rust-studio
XDND=$ROOT/docs/test-reports/evidence/issue-0030/xdnd_source.py
OUT=/home/hatch/demucs-work/gui101
mkdir -p "$OUT"
cp /home/hatch/demucs-work/melody_only.wav "$OUT/melody_only.wav"
# 配置先トラックのあるプロジェクトをバイナリ自身に作らせる（#99 と同じ種）
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
python3 "$XDND" daw 400 200 "file://$OUT/melody_only.wav" > "$OUT/xdnd.txt" 2>&1
sleep 1
grep -q "separation started:" "$OUT/gui-stdout.txt" || { echo "FAIL: separation did not start"; cleanup; exit 1; }
for i in $(seq 1 120); do grep -q "separated:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 1; done
grep -q "separated:" "$OUT/gui-stdout.txt" || { echo "FAIL: separation did not finish"; cleanup; exit 1; }
sleep 1
ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :98 -frames:v 1 "$OUT/shot1-timeline.png"
# 下へスクロールして 07 TONES パネルを出す
xdotool mousemove 480 300
for i in 1 2 3 4 5 6; do xdotool click 5; sleep 0.3; done
sleep 1
ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :98 -frames:v 1 "$OUT/shot2-tones.png"
# 再生開始（Space）→ 再生ヘッドが進むこと
xdotool key space
sleep 2
ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :98 -frames:v 1 "$OUT/shot3-playing.png"
xdotool key space
cleanup
python3 - "$OUT" <<'PYEOF'
import json, sys
out = sys.argv[1]
p = json.load(open(f"{out}/project.json"))
assert len(p["tones"]) == 4, p["tones"]
tones = {t["id"]: t for t in p["tones"]}
stem_tracks = [t for t in p["tracks"] if t["name"].startswith("melody_only /")]
assert len(stem_tracks) == 4, [t["name"] for t in p["tracks"]]
for t in stem_tracks:
    assert t["tone_id"] in tones, t
    assert tones[t["tone_id"]]["instrument"]["type"] == "sampler"
other = next(t for t in stem_tracks if t["name"].endswith("/ other"))
pitches = [n["pitch"] for c in other["clips"] for n in c["notes"]]
assert pitches == [69, 72, 76, 81], pitches
assert tones[other["tone_id"]]["instrument"]["root_note"] == 69
print("GUI TONE CHECK OK: 4 tones registered and assigned; other notes", pitches)
PYEOF
