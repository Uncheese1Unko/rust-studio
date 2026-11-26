#!/usr/bin/env python3
"""Issue #101 の MCP 駆動: 分離済みプロジェクトファイルを
studio-mcp --project-file で開き、get_project のダンプに
トラック・ノート・音色・割り当てが同じ形で現れることを確認する。
使い方: mcp_dump.py <studio-mcp binary> <project.json>"""
import json
import subprocess
import sys

MCP, PROJECT = sys.argv[1], sys.argv[2]
proc = subprocess.Popen(
    [MCP, "--project-file", PROJECT],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

REQ = [100]


def send(obj):
    proc.stdin.write(json.dumps(obj) + "\n")
    proc.stdin.flush()
    return json.loads(proc.stdout.readline())


def call(name, arguments):
    REQ[0] += 1
    r = send({"jsonrpc": "2.0", "id": REQ[0], "method": "tools/call",
              "params": {"name": name, "arguments": arguments}})
    assert not r["result"].get("isError"), (name, r)
    return r


send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                 "clientInfo": {"name": "drive101", "version": "0"}}})
proc.stdin.write(json.dumps({"jsonrpc": "2.0",
                             "method": "notifications/initialized",
                             "params": {}}) + "\n")
proc.stdin.flush()

r = call("get_project", {})
proj = json.loads(r["result"]["content"][0]["text"])
proc.stdin.close()
proc.wait(timeout=10)

on_disk = json.load(open(PROJECT))
assert proj == on_disk, "MCP dump differs from the project file"
tones = {t["id"]: t for t in proj["tones"]}
assert len(tones) == 4, tones
for track in proj["tracks"]:
    assert track["tone_id"] in tones, track
    inst = tones[track["tone_id"]]["instrument"]
    assert inst["type"] == "sampler", inst
other = next(t for t in proj["tracks"] if t["name"].endswith("/ other"))
pitches = [n["pitch"] for c in other["clips"] for n in c["notes"]]
assert pitches == [69, 72, 76, 81], pitches
root = tones[other["tone_id"]]["instrument"]["root_note"]
assert root == 69, root
print("MCP DUMP CHECK OK: 4 sampler tones, tone_id assignments,",
      f"other notes {pitches}, root_note {root}")
