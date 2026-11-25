#!/usr/bin/env python3
"""Issue #95 の MCP 駆動: studio-mcp 経由でエフェクトを追加・
バイパスし、プロジェクトファイルに永続化されること、不正な
コマンドがエラーになり状態が変わらないことを確認する。
音の測定は drive.sh が書き出し WAV で行う。
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


def send(obj):
    proc.stdin.write(json.dumps(obj) + "\n")
    proc.stdin.flush()
    return json.loads(proc.stdout.readline())


def call(name, arguments, req_id):
    return send({"jsonrpc": "2.0", "id": req_id, "method": "tools/call",
                 "params": {"name": name, "arguments": arguments}})


def get_effects(req_id):
    r = call("get_project", {}, req_id)
    proj = json.loads(r["result"]["content"][0]["text"])
    return proj["tracks"][0]["effects"], r["result"]["content"][0]["text"]


send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                 "clientInfo": {"name": "drive95", "version": "0"}}})
proc.stdin.write(json.dumps({"jsonrpc": "2.0",
                             "method": "notifications/initialized",
                             "params": {}}) + "\n")
proc.stdin.flush()

r = call("add_track_effect",
         {"track_id": 1, "effect": {"type": "delay", "time_ms": 200,
                                    "feedback": 0.5, "mix": 0.5}}, 2)
assert not r["result"].get("isError"), r
effects, _ = get_effects(3)
print("after add:", effects)
assert effects == [{"effect": {"type": "delay", "time_ms": 200,
                               "feedback": 0.5, "mix": 0.5},
                    "bypassed": False}], effects

r = call("set_track_effect_bypass",
         {"track_id": 1, "index": 0, "bypassed": True}, 4)
assert not r["result"].get("isError"), r
effects, _ = get_effects(5)
assert effects[0]["bypassed"] is True, effects
print("bypass on OK")
r = call("set_track_effect_bypass",
         {"track_id": 1, "index": 0, "bypassed": False}, 6)
assert not r["result"].get("isError"), r
effects, before = get_effects(7)
assert effects[0]["bypassed"] is False, effects
print("bypass off OK")

# 不正なコマンド: 範囲外パラメータと範囲外インデックスは
# エラーになり、ダンプは変わらない
r = call("add_track_effect",
         {"track_id": 1, "effect": {"type": "eq3", "low_gain_db": 99.0,
                                    "mid_gain_db": 0.0,
                                    "high_gain_db": 0.0}}, 8)
assert r["result"].get("isError"), r
r = call("remove_track_effect", {"track_id": 1, "index": 9}, 9)
assert r["result"].get("isError"), r
_, after = get_effects(10)
assert before == after, "dump changed after rejected commands"
print("rejected commands left the project unchanged OK")

proc.stdin.close()
proc.terminate()
saved = json.load(open(f"{WORK}/project.json"))
assert saved["tracks"][0]["effects"] == effects, saved["tracks"][0]
print("project file persisted OK")
print("MCP FX OK")
