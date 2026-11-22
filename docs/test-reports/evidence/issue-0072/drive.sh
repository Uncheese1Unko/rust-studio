#!/bin/bash
# Issue #72 acceptance drive:
# 1) CLI 書き出し: フィクスチャ CLAP プラグインのトラックが
#    プリセット（gain 0.25 / 矩形波）付きで鳴り、壊れたプラグインの
#    トラックは stderr に明示エラー、書き出し自体は成功する。
# 2) GUI: Xvfb + lavapipe で起動し、プラグインエラーが stdout と
#    画面（トランスポート）に出て、アプリが落ちないことを確認する。
# Vital 実バイナリはアカウント経由配布のため未入手。代替として
# ワークスペース内のテスト用 CLAP シンセ（fixtures/clap-test-synth）
# でパイプライン全体を実証する。
set -u
export DISPLAY=:96
ROOT=/home/hatch/workspace/repos/rust-studio
OUT=$ROOT/docs/test-reports/evidence/issue-0072
FIXTURE=$(find "$ROOT/target/debug" -name "libclap_test_synth.so" | head -1)
[ -n "$FIXTURE" ] || { echo "FAIL: fixture .so not built"; exit 1; }
LIVE=/tmp/issue0072-project.json
sed -e "s|__FIXTURE__|$FIXTURE|" -e "s|__PRESET__|$OUT/preset.vital|" \
  "$OUT/project.seed.json" > "$LIVE"

# --- 1) CLI 書き出し ---
"$ROOT/target/debug/daw" --export-wav "$LIVE" /tmp/issue0072-export.wav \
  > "$OUT/export-stdout.txt" 2> "$OUT/export-stderr.txt"
echo "export exit: $?" >> "$OUT/export-stdout.txt"
grep -q "plugin error \[BROKEN\]" "$OUT/export-stderr.txt" \
  || { echo "FAIL: broken track error not reported"; exit 1; }
python3 - /tmp/issue0072-export.wav <<'PYEOF' || exit 1
import struct, sys

def read_wav(path):
    data = open(path, 'rb').read()
    # hound の書き出しは標準 WAV。data チャンクを探す。
    at = data.find(b'data')
    assert at >= 0
    size = struct.unpack('<I', data[at+4:at+8])[0]
    raw = data[at+8:at+8+size]
    vals = struct.unpack('<%dh' % (len(raw)//2), raw)
    return [v / 32768.0 for v in vals[0::2]], 44100

def goertzel(samples, rate, freq):
    import math
    k = 2 * math.pi * freq / rate
    coeff = 2 * math.cos(k)
    s1 = s2 = 0.0
    for x in samples:
        s = x + coeff * s1 - s2
        s2, s1 = s1, s
    return math.sqrt(max(s1*s1 + s2*s2 - coeff*s1*s2, 0.0)) / len(samples) * 2

samples, rate = read_wav(sys.argv[1])
first = samples[int(0.3*rate):int(0.8*rate)]   # ノート 69 (440Hz)
second = samples[int(1.3*rate):int(1.8*rate)]  # ノート 72 (523Hz)
peak = max(abs(x) for x in first)
assert goertzel(first, rate, 440.0) > 0.05, "note 69 must sound at 440Hz"
assert goertzel(second, rate, 523.25) > 0.05, "note 72 must sound at 523Hz"
assert abs(peak - 0.25) < 0.04, f"preset gain 0.25 must apply, peak={peak}"
print(f"export OK: 440Hz + 523Hz present, peak {peak:.3f} (preset gain)")
PYEOF

# --- 2) GUI 駆動 ---
Xvfb :96 -screen 0 960x540x24 > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
sleep 1
STUDIO_PROJECT_FILE="$LIVE" "$ROOT/target/debug/daw" \
  > "$OUT/gui-stdout.txt" 2> "$OUT/gui-stderr.txt" &
GUI_PID=$!
for i in $(seq 1 60); do grep -q "elements:" "$OUT/gui-stdout.txt" 2>/dev/null && break; sleep 0.5; done
grep -q "plugin error \[BROKEN\]" "$OUT/gui-stdout.txt" \
  || { echo "FAIL: GUI did not report the plugin error"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
shot() { ffmpeg -y -loglevel error -draw_mouse 0 -f x11grab -video_size 960x540 -i :96 -frames:v 1 "$OUT/$1"; }
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
sleep 1
shot shot1-timeline-plugin-error.png
kill -0 $GUI_PID 2>/dev/null || { echo "FAIL: GUI died"; kill $XVFB_PID 2>/dev/null; exit 1; }
# トランスポートのエラー表示は ACCENT (#FF2E88) で描かれる。
# 実測域（x 500..730, y 304..320）にピンク画素があること。
# 小さい文字のアンチエイリアスで淡くなるため閾値は緩めにする。
python3 - "$OUT/shot1-timeline-plugin-error.png" <<'PYEOF' || { echo "FAIL: no accent pixels in error area"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
from PIL import Image
import sys
im = Image.open(sys.argv[1]).convert('RGB')
hits = 0
for y in range(304, 320):
    for x in range(500, 730):
        r, g, b = im.getpixel((x, y))
        if r > 190 and g < 185 and 110 < b < 215 and r - b > 35:
            hits += 1
print(f"accent pixels in error band: {hits}")
sys.exit(0 if hits > 100 else 1)
PYEOF

# --- 3) 外部変更（監視経路）: 起動中にプロジェクトファイルを
#         書き換えて壊れたプラグインパスを直し、再読込後に
#         エラー表示が消えることを確認する（Issue #72 レビュー
#         Major の再現テスト: 再読込だけでは事前レンダリングと
#         エラー状態が更新されなかった経路）。
python3 - "$LIVE" "$FIXTURE" <<'PYEOF'
import json, sys
path, plugin = sys.argv[1], sys.argv[2]
with open(path) as f:
    project = json.load(f)
for track in project["tracks"]:
    inst = track.get("instrument")
    if isinstance(inst, dict) and inst.get("type") == "plugin":
        if "nonexistent" in inst.get("plugin_path", ""):
            inst["plugin_path"] = plugin
with open(path, "w") as f:
    json.dump(project, f)
PYEOF
for i in $(seq 1 40); do grep -q "project reloaded:" "$OUT/gui-stdout.txt" && break; sleep 0.5; done
grep -q "project reloaded:" "$OUT/gui-stdout.txt" \
  || { echo "FAIL: watch reload did not happen"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
sleep 3
shot shot2-after-external-fix.png
python3 - "$OUT/shot1-timeline-plugin-error.png" "$OUT/shot2-after-external-fix.png" <<'PYEOF' || { echo "FAIL: error display did not clear"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
from PIL import Image
import sys
def hits(path):
    im = Image.open(path).convert('RGB')
    n = 0
    for y in range(304, 320):
        for x in range(500, 730):
            r, g, b = im.getpixel((x, y))
            if r > 190 and g < 185 and 110 < b < 215 and r - b > 35:
                n += 1
    return n
before, after = hits(sys.argv[1]), hits(sys.argv[2])
print(f"accent pixels in error band before/after external fix: {before} -> {after}")
sys.exit(0 if after < 20 else 1)
PYEOF
[ "$(grep -c 'plugin error \[BROKEN\]' "$OUT/gui-stdout.txt")" = "1" ] \
  || { echo "FAIL: broken error re-reported or missing"; kill $GUI_PID $XVFB_PID 2>/dev/null; exit 1; }
kill $GUI_PID 2>/dev/null
kill $XVFB_PID 2>/dev/null
echo "DRIVE OK"
