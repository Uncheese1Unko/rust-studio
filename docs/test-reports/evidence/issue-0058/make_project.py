#!/usr/bin/env python3
"""Issue #58 の証拠用プロジェクトを作る。

デモの初期ダンプと同じ形式で、再生位置の制御ができる配置にする:
- main: 0..15000ms（開始直後はこのクリップが再生中）
- sub:  20000..50000ms（プロジェクト終端＝ラップは 50 秒後）
"""
import json

project = {
    "name": "color-evidence",
    "next_id": 3,
    "tracks": [
        {
            "id": 1,
            "name": "A1",
            "kind": "audio",
            "volume": 0.8,
            "pan": 0.0,
            "clips": [
                {
                    "id": 1,
                    "name": "main",
                    "start_ms": 0,
                    "duration_ms": 15000,
                    "source": "/tmp/rust-studio-daw-media/tone220.wav",
                    "gain": 1.0,
                },
                {
                    "id": 2,
                    "name": "sub",
                    "start_ms": 20000,
                    "duration_ms": 30000,
                    "source": "/tmp/rust-studio-daw-media/tone440.wav",
                    "gain": 1.0,
                },
            ],
        }
    ],
}

with open("/tmp/issue58-project.json", "w") as f:
    json.dump(project, f, indent=2)
print("wrote /tmp/issue58-project.json")
