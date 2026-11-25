#!/usr/bin/env python3
"""Issue #95 の HTTP API 駆動: studio-api 経由でエフェクトを
追加・バイパスし、GET /project に現れること、不正なコマンドが
400 で状態不変なこと、/undo で巻き戻せることを確認する。
使い方: api_fx.py <studio-api binary>"""
import json
import subprocess
import sys
import time
import urllib.request
import urllib.error

API = sys.argv[1]
ADDR = "127.0.0.1:17899"
BASE = f"http://{ADDR}"

proc = subprocess.Popen([API, ADDR], stdout=subprocess.PIPE,
                        stderr=subprocess.STDOUT, text=True)


def request(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, method=method,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=5) as res:
            return res.status, res.read().decode()
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()


try:
    for _ in range(50):
        try:
            status, _ = request("GET", "/project")
            break
        except Exception:
            time.sleep(0.2)
    else:
        raise RuntimeError("studio-api did not start")

    status, body = request("POST", "/commands",
                           {"type": "add_track", "name": "A1",
                            "kind": "audio"})
    assert status == 200, (status, body)
    track_id = json.loads(body)["created_id"]
    print("track created:", track_id)

    status, body = request(
        "POST", "/commands",
        {"type": "add_track_effect", "track_id": track_id,
         "effect": {"type": "eq3", "low_gain_db": 6.0, "mid_gain_db": 0.0,
                    "high_gain_db": 0.0}})
    assert status == 200, (status, body)
    status, body = request("GET", "/project")
    proj = json.loads(body)
    effects = proj["tracks"][0]["effects"]
    print("after add:", effects)
    assert effects == [{"effect": {"type": "eq3", "low_gain_db": 6.0,
                                   "mid_gain_db": 0.0, "high_gain_db": 0.0},
                        "bypassed": False}], effects

    status, body = request(
        "POST", "/commands",
        {"type": "set_track_effect_bypass", "track_id": track_id,
         "index": 0, "bypassed": True})
    assert status == 200, (status, body)
    status, before = request("GET", "/project")
    assert json.loads(before)["tracks"][0]["effects"][0]["bypassed"] is True
    print("bypass on OK")

    # 不正なコマンドは 400 で、状態は変わらない
    status, body = request(
        "POST", "/commands",
        {"type": "add_track_effect", "track_id": track_id,
         "effect": {"type": "delay", "time_ms": 200, "feedback": 2.0,
                    "mix": 0.5}})
    assert status == 400, (status, body)
    status, after = request("GET", "/project")
    assert before == after, "project changed after rejected command"
    print("rejected command left the project unchanged OK")

    # Undo でバイパスが巻き戻る
    status, body = request("POST", "/undo")
    assert status == 200 and json.loads(body)["undone"] is True, body
    status, body = request("GET", "/project")
    effects = json.loads(body)["tracks"][0]["effects"]
    assert effects[0]["bypassed"] is False, effects
    print("undo OK")
finally:
    proc.terminate()

print("API FX OK")
