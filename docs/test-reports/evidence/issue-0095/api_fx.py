#!/usr/bin/env python3
"""Issue #95 の HTTP API 駆動: studio-api 経由で MCP と同じ
コマンド列を適用し、最終チェーンが MCP 経路と一致すること
（FINAL_EFFECTS 行で drive.sh が突き合わせる）、不正なコマンド
（範囲外パラメータ／範囲外インデックス／存在しないトラック）が
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


def get_effects():
    status, body = request("GET", "/project")
    assert status == 200, (status, body)
    return json.loads(body)["tracks"][0]["effects"], body


DELAY = {"type": "delay", "time_ms": 200, "feedback": 0.5, "mix": 0.5}
EQ3 = {"type": "eq3", "low_gain_db": 6.0, "mid_gain_db": 0.0,
       "high_gain_db": 0.0}


def slot(effect, bypassed=False):
    return {"effect": effect, "bypassed": bypassed}


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

    # MCP 経路と同じコマンド列（add delay → add eq3 → bypass eq3）
    for cmd in [
        {"type": "add_track_effect", "track_id": track_id, "effect": DELAY},
        {"type": "add_track_effect", "track_id": track_id, "effect": EQ3},
        {"type": "set_track_effect_bypass", "track_id": track_id,
         "index": 1, "bypassed": True},
    ]:
        status, body = request("POST", "/commands", cmd)
        assert status == 200, (status, body, cmd)
    effects, before = get_effects()
    print("after commands:", effects)
    assert effects == [slot(DELAY), slot(EQ3, True)], effects
    print("FINAL_EFFECTS:", json.dumps(effects, sort_keys=True))

    # 不正なコマンド3種は 400 で、状態は変わらない
    for cmd in [
        {"type": "add_track_effect", "track_id": track_id,
         "effect": {"type": "delay", "time_ms": 200, "feedback": 2.0,
                    "mix": 0.5}},
        {"type": "remove_track_effect", "track_id": track_id, "index": 9},
        {"type": "add_track_effect", "track_id": 999, "effect": DELAY},
    ]:
        status, body = request("POST", "/commands", cmd)
        assert status == 400, (status, body, cmd)
    _, after = get_effects()
    assert before == after, "project changed after rejected commands"
    print("rejected commands left the project unchanged OK")

    # Undo で最後の成功コマンド（バイパス）が巻き戻る
    status, body = request("POST", "/undo")
    assert status == 200 and json.loads(body)["undone"] is True, body
    effects, _ = get_effects()
    assert effects == [slot(DELAY), slot(EQ3)], effects
    print("undo OK")
finally:
    proc.terminate()

print("API FX OK")
