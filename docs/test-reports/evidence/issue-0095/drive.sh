#!/usr/bin/env bash
# Issue #95 の駆動証拠: MCP と HTTP API の両経路でエフェクトを
# 操作し、MCP で作った状態を `daw --export-wav` で書き出して
# ディレイのエコーを測定する。
# 使い方: bash drive.sh [REPO_ROOT]
set -u
set -o pipefail
ROOT="${1:-$(cd "$(dirname "$0")/../../../.." && pwd)}"
HERE="$(cd "$(dirname "$0")" && pwd)"
DAW="$ROOT/target/debug/daw"
MCP="$ROOT/target/debug/studio-mcp"
API="$ROOT/target/debug/studio-api"
W=/tmp/issue0095
mkdir -p "$W"
exec > >(tee "$HERE/drive-log.txt") 2>&1

echo "== MCP: all five tools / bypass / reject =="
python3 "$HERE/mcp_fx.py" "$MCP" "$W" | tee "$W/mcp.out" || { echo "FAIL: mcp"; exit 1; }

echo "== export the MCP-driven project and measure the delay =="
(cd "$W" && "$DAW" --export-wav project.json out.wav) || { echo "FAIL: export"; exit 1; }
python3 - "$W/out.wav" << 'PYEOF' || { echo "FAIL: measure"; exit 1; }
import struct, sys
d = open(sys.argv[1], "rb").read()
n = struct.unpack("<I", d[40:44])[0]
s = [struct.unpack_from("<h", d, 44 + i * 2)[0] / 32768 for i in range(n // 2)]
left = s[0::2]
sr = 44100
def at(ms):
    return left[int(sr * ms / 1000)]
dry = at(0)
e1, e2, e3 = at(200), at(400), at(600)
print(f"dry {dry:.4f} echo@200 {e1:.4f} echo@400 {e2:.4f} echo@600 {e3:.4f}")
assert abs(e1 / dry - 0.5) < 0.03, e1 / dry
assert abs(e2 / dry - 0.25) < 0.03, e2 / dry
assert abs(e3 / dry - 0.125) < 0.03, e3 / dry
assert at(100) == 0.0
print("delay measurement OK (ratios 0.5 / 0.25 / 0.125 of dry)")
PYEOF

echo "== HTTP API: same commands / reject / undo =="
python3 "$HERE/api_fx.py" "$API" | tee "$W/api.out" || { echo "FAIL: api"; exit 1; }

echo "== cross-route: the same command sequence yields the same chain =="
python3 - "$W/mcp.out" "$W/api.out" << 'PYEOF' || { echo "FAIL: cross-route"; exit 1; }
import json, sys
def final_effects(path):
    for line in open(path):
        if line.startswith("FINAL_EFFECTS: "):
            return json.loads(line[len("FINAL_EFFECTS: "):])
    raise AssertionError(f"no FINAL_EFFECTS in {path}")
mcp = final_effects(sys.argv[1])
api = final_effects(sys.argv[2])
print("MCP final:", mcp)
print("API final:", api)
assert mcp == api, "routes disagree on the same command sequence"
print("cross-route match OK")
PYEOF

echo "DRIVE OK"
