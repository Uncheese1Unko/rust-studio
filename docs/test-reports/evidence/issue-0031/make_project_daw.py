#!/usr/bin/env python3
"""Issue #31 DAW 証拠用プロジェクト: 1トラック、移動対象クリップ1件。"""
import json, sys
project = {
    "name": "undo-evidence-daw",
    "next_id": 3,
    "tracks": [
        {
            "id": 1, "name": "A1", "kind": "audio", "volume": 0.8, "pan": 0.0,
            "clips": [
                {"id": 1, "name": "move", "start_ms": 1000, "duration_ms": 2000,
                 "source": sys.argv[1], "gain": 1.0}
            ],
        }
    ],
}
open(sys.argv[2], "w").write(json.dumps(project))
