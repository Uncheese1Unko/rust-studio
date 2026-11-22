#!/bin/bash
# Issue #51 証拠ドライブ: ゲインスライダーで選択中クリップの音量を
# 変え、クリップ上端コーナーのドラッグでフェードを設定し、
# タイムラインに傾斜が描かれることを確認する。
# Xvfb(:92) + lavapipe 前提（実ディスプレイ未検証）。
set -u
export DISPLAY=:92
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
BIN=~/workspace/repos/rust-studio-lane-b/target/debug/video-editor
OUT=/tmp/issue51
mkdir -p "$OUT"

[ -S /tmp/.X11-unix/X92 ] || { setsid nohup Xvfb :92 -screen 0 960x720x24 > /tmp/xvfb92.log 2>&1 < /dev/null & sleep 2; }

shot() {
  ffmpeg -y -loglevel error -f x11grab -draw_mouse 0 -video_size 960x720 -i :92 -frames:v 1 "$1"
}
hold_drag() { # x1 y1 x2 y2
  xdotool mousemove $1 $2 mousedown 1; sleep 0.35
  xdotool mousemove $3 $4; sleep 0.35
  xdotool mouseup 1
}

python3 - <<'EOF'
import json
def clip(cid, name, start, dur, source):
    return {"id": cid, "name": name, "start_ms": start, "duration_ms": dur,
            "source": source, "offset_ms": 0, "gain": 1.0,
            "pitch_semitones": 0.0, "cutout": False, "color": None,
            "fade_in_ms": 0, "fade_out_ms": 0, "title_text": "",
            "transition_in_kind": "none", "transition_in_ms": 0}
proj = {"name": "issue51", "tracks": [
    {"id": 1, "name": "V1", "kind": "video", "volume": 1.0, "pan": 0.0, "color": None,
     "clips": [clip(3, "scene", 0, 8000, "demo/opening.mp4")]},
    {"id": 2, "name": "A1", "kind": "audio", "volume": 1.0, "pan": 0.0, "color": None,
     "clips": [clip(4, "bgm", 0, 8000, "demo/bgm.wav")]}],
    "next_id": 5}
open("/tmp/issue51/project.json", "w").write(json.dumps(proj, indent=2))
print("project written")
EOF

STUDIO_PROJECT_FILE=/tmp/issue51/project.json "$BIN" > "$OUT/stdout.log" 2>&1 &
APP=$!
for i in $(seq 1 60); do grep -q "preview textures loaded" "$OUT/stdout.log" && break; sleep 0.5; done
sleep 0.5
shot "$OUT/01-initial.png"

# A1 のヘッダーをクリックして選択（対象クリップ = bgm にする）
xdotool mousemove 60 96 mousedown 1; sleep 0.3; xdotool mouseup 1
sleep 0.5

# ゲインスライダー: 中央（720, 448）から左（610）へドラッグ
# → 割合 0.25 → ゲイン 0.5
hold_drag 720 448 610 448
for i in $(seq 1 20); do grep -q "after gain:" "$OUT/stdout.log" && break; sleep 0.5; done
python3 - <<'EOF'
import json
proj = json.load(open("/tmp/issue51/project.json"))
bgm = proj["tracks"][1]["clips"][0]
print(f"bgm gain after slider drag: {bgm['gain']}")
assert abs(bgm["gain"] - 0.5) < 0.03, bgm
print("OK gain slider set bgm gain to 0.5")
EOF
shot "$OUT/02-gain-slider.png"

# フェードイン: V1 クリップの左上コーナー（122, 28）を右へ 42px
hold_drag 122 28 164 28
for i in $(seq 1 20); do grep -q "after drag:" "$OUT/stdout.log" && break; sleep 0.5; done
# フェードアウト: 右上コーナー（790, 28）を左へ 42px
hold_drag 790 28 748 28
sleep 1
python3 - <<'EOF'
import json
proj = json.load(open("/tmp/issue51/project.json"))
scene = proj["tracks"][0]["clips"][0]
print(f"scene fades: in={scene['fade_in_ms']} out={scene['fade_out_ms']}")
assert abs(scene["fade_in_ms"] - 500) <= 30, scene
assert abs(scene["fade_out_ms"] - 500) <= 30, scene
print("OK corner drags set fade_in/fade_out to ~500ms")
EOF
shot "$OUT/03-fade-slopes.png"

kill $APP 2>/dev/null; wait $APP 2>/dev/null
echo "DRIVE51_DONE"
