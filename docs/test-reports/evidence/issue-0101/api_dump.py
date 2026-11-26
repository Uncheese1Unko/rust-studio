#!/usr/bin/env python3
"""Issue #101 の HTTP API 駆動: studio-api はプロジェクトファイルの
読み込みを持たない（空のプロジェクトから始まる）ため、分離結果そのものは
読み込めない。その代わり、音色登録と割り当てが API のコマンドと
GET /project ダンプでも同じ形（tones／tone_id／sampler）で
観測できることを駆動で示す。
使い方: api_dump.py <studio-api binary>"""
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
                           {"type": "add_track", "name": "song / other",
                            "kind": "audio"})
    assert status == 200, (status, body)
    track_id = json.loads(body)["created_id"]
    status, body = request("POST", "/commands",
                           {"type": "add_tone", "name": "song / other",
                            "instrument": {"type": "sampler",
                                           "source": "/stems/song/other.wav",
                                           "root_note": 69}})
    assert status == 200, (status, body)
    tone_id = json.loads(body)["created_id"]
    status, body = request("POST", "/commands",
                           {"type": "set_track_tone", "track_id": track_id,
                            "tone_id": tone_id})
    assert status == 200, (status, body)

    status, body = request("GET", "/project")
    assert status == 200, (status, body)
    proj = json.loads(body)
    assert len(proj["tones"]) == 1, proj["tones"]
    tone = proj["tones"][0]
    assert tone["instrument"] == {"type": "sampler",
                                  "source": "/stems/song/other.wav",
                                  "root_note": 69}, tone
    assert proj["tracks"][0]["tone_id"] == tone["id"], proj["tracks"]
    print("API DUMP CHECK OK: tones and tone_id observable in the same",
          "dump shape via GET /project")
finally:
    proc.terminate()
