#!/usr/bin/env python3
"""Issue #89 の回帰確認: 取り込んだトラックの楽器を MCP の
set_track_instrument でサンプラーへ差し替えても鳴ること。
Lead を 220Hz サイン素材・ルート 60 のサンプラーに差し替えると、
ノート 69 は +9 半音で 220*2^(9/12)=369.99Hz、ノート 72 は
+12 半音で 440Hz になるはず（シンセの 440/523.25 ではなく）。
作業フォルダは /tmp/issue0089（drive.sh が用意する）。
使い方: mcp_replace.py <studio-mcp binary>"""
import json
import math
import struct
import subprocess
import sys
import wave

WORK = "/tmp/issue0089"
MCP = sys.argv[1]

# 220Hz サイン素材を作る
rate = 44100
frames = []
for i in range(rate * 3):
    v = int(0.4 * 32767 * math.sin(2 * math.pi * 220.0 * i / rate))
    frames.append(struct.pack("<h", v))
w = wave.open(f"{WORK}/tone220.wav", "wb")
w.setnchannels(1)
w.setsampwidth(2)
w.setframerate(rate)
w.writeframes(b"".join(frames))
w.close()

proc = subprocess.Popen(
    [MCP, "--project-file", f"{WORK}/project2.json"], stdin=subprocess.PIPE,
    stdout=subprocess.PIPE, text=True)


def send(obj):
    proc.stdin.write(json.dumps(obj) + "\n")
    proc.stdin.flush()
    return json.loads(proc.stdout.readline())


send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                 "clientInfo": {"name": "drive89", "version": "0"}}})
proc.stdin.write(json.dumps({"jsonrpc": "2.0",
                             "method": "notifications/initialized",
                             "params": {}}) + "\n")
proc.stdin.flush()
r = send({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
          "params": {"name": "set_track_instrument",
                     "arguments": {"track_id": 1,
                                   "instrument": {
                                       "type": "sampler",
                                       "source": f"{WORK}/tone220.wav",
                                       "root_note": 60}}}})
print("set_track_instrument raw:", r)
assert not r["result"].get("isError"), r
r = send({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
          "params": {"name": "get_project", "arguments": {}}})
proj = json.loads(r["result"]["content"][0]["text"])
lead, bass = proj["tracks"][0], proj["tracks"][1]
print("Lead instrument:", lead["instrument"])
print("Bass instrument:", bass["instrument"])
assert lead["instrument"]["type"] == "sampler", lead["instrument"]
assert bass["instrument"]["type"] == "synth", bass["instrument"]
proc.stdin.close()
proc.terminate()
print("MCP REPLACE OK")
