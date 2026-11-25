#!/usr/bin/env python3
"""Issue #95 の MCP 駆動: studio-mcp の5ツール（add / remove /
move / set / set_bypass）すべてを成功系で駆動し、get_project に
現れること、不正なコマンド（範囲外パラメータ／範囲外
インデックス／存在しないトラック）がエラーで状態不変なこと、
プロジェクトファイルへの永続化を確認する。最後に API 経路と
突き合わせるための正準チェーンを作り、FINAL_EFFECTS 行を出す。
使い方: mcp_fx.py <studio-mcp binary> <work dir>"""
import json
import struct
import subprocess
import sys

MCP = sys.argv[1]
WORK = sys.argv[2]

# インパルス素材（44.1kHz mono 2秒、先頭だけ最大振幅）
sr = 44100
frames = sr * 2
samples = struct.pack("<h", 32767) + b"\x00\x00" * (frames - 1)
hdr = (b"RIFF" + struct.pack("<I", 36 + len(samples)) + b"WAVEfmt "
       + struct.pack("<IHHIIHH", 16, 1, 1, sr, sr * 2, 2, 16)
       + b"data" + struct.pack("<I", len(samples)))
open(f"{WORK}/imp.wav", "wb").write(hdr + samples)

project = {
    "version": 1, "name": "fx95", "sample_rate": 44100, "next_id": 10,
    "tones": [],
    "tracks": [{"id": 1, "name": "A1", "kind": "audio", "volume": 0.8,
                "pan": 0.0, "instrument": None,
                "clips": [{"id": 1, "name": "impulse", "start_ms": 0,
                           "duration_ms": 2000, "source": "imp.wav",
                           "offset_ms": 0, "gain": 1.0, "fade_in_ms": 0,
                           "fade_out_ms": 0, "script_effect": None}],
                "tone_id": None, "effects": []}],
    "master_volume": 0.8, "playhead_ms": 0, "loop_region": None,
    "markers": [], "tempo_bpm": 120.0,
}
json.dump(project, open(f"{WORK}/project.json", "w"))

proc = subprocess.Popen(
    [MCP, "--project-file", f"{WORK}/project.json"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

REQ = [100]


def send(obj):
    proc.stdin.write(json.dumps(obj) + "\n")
    proc.stdin.flush()
    return json.loads(proc.stdout.readline())


def call(name, arguments, expect_error=False):
    REQ[0] += 1
    r = send({"jsonrpc": "2.0", "id": REQ[0], "method": "tools/call",
              "params": {"name": name, "arguments": arguments}})
    is_error = bool(r["result"].get("isError"))
    assert is_error == expect_error, (name, arguments, r)
    return r


def get_effects():
    r = call("get_project", {})
    proj = json.loads(r["result"]["content"][0]["text"])
    return proj["tracks"][0]["effects"], r["result"]["content"][0]["text"]


send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                 "clientInfo": {"name": "drive95", "version": "0"}}})
proc.stdin.write(json.dumps({"jsonrpc": "2.0",
                             "method": "notifications/initialized",
                             "params": {}}) + "\n")
proc.stdin.flush()

DELAY = {"type": "delay", "time_ms": 200, "feedback": 0.5, "mix": 0.5}
EQ3 = {"type": "eq3", "low_gain_db": 6.0, "mid_gain_db": 0.0,
       "high_gain_db": 0.0}
REVERB = {"type": "reverb", "decay": 0.5, "mix": 0.3}


def slot(effect, bypassed=False):
    return {"effect": effect, "bypassed": bypassed}


# add（成功系）
call("add_track_effect", {"track_id": 1, "effect": DELAY})
effects, _ = get_effects()
assert effects == [slot(DELAY)], effects
print("add OK:", effects)

# set_bypass の往復（成功系）
call("set_track_effect_bypass", {"track_id": 1, "index": 0, "bypassed": True})
effects, _ = get_effects()
assert effects == [slot(DELAY, True)], effects
call("set_track_effect_bypass", {"track_id": 1, "index": 0, "bypassed": False})
effects, _ = get_effects()
assert effects == [slot(DELAY)], effects
print("set_bypass roundtrip OK")

# add 2つ目 → move（成功系）で順序が入れ替わる
call("add_track_effect", {"track_id": 1, "effect": EQ3})
call("move_track_effect", {"track_id": 1, "from_index": 1, "to_index": 0})
effects, _ = get_effects()
assert effects == [slot(EQ3), slot(DELAY)], effects
print("move OK:", effects)

# set（成功系）で種類ごと差し替わる（バイパス保持は #92 の単体
# テストで担保。ここでの差し替え対象はバイパスなしのスロット）
call("set_track_effect", {"track_id": 1, "index": 0, "effect": REVERB})
effects, _ = get_effects()
assert effects == [slot(REVERB), slot(DELAY)], effects
print("set OK:", effects)

# remove（成功系）
call("remove_track_effect", {"track_id": 1, "index": 0})
effects, before = get_effects()
assert effects == [slot(DELAY)], effects
print("remove OK:", effects)

# 不正なコマンド3種: 範囲外パラメータ／範囲外インデックス／
# 存在しないトラック。すべて isError で、ダンプは変わらない。
call("add_track_effect",
     {"track_id": 1, "effect": {"type": "eq3", "low_gain_db": 99.0,
                                "mid_gain_db": 0.0, "high_gain_db": 0.0}},
     expect_error=True)
call("remove_track_effect", {"track_id": 1, "index": 9}, expect_error=True)
call("add_track_effect", {"track_id": 999, "effect": DELAY},
     expect_error=True)
call("set_track_effect_bypass",
     {"track_id": 999, "index": 0, "bypassed": True}, expect_error=True)
_, after = get_effects()
assert before == after, "dump changed after rejected commands"
print("rejected commands left the project unchanged OK")

# API 経路と突き合わせる正準チェーン: [delay（有効）, eq3（バイパス）]
call("add_track_effect", {"track_id": 1, "effect": EQ3})
call("set_track_effect_bypass", {"track_id": 1, "index": 1, "bypassed": True})
effects, _ = get_effects()
assert effects == [slot(DELAY), slot(EQ3, True)], effects
print("FINAL_EFFECTS:", json.dumps(effects, sort_keys=True))

proc.stdin.close()
proc.terminate()
saved = json.load(open(f"{WORK}/project.json"))
assert saved["tracks"][0]["effects"] == effects, saved["tracks"][0]
print("project file persisted OK")
print("MCP FX OK")
