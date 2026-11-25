#!/usr/bin/env python3
"""Issue #92: daw --export-wav で書き出した WAV の帯域ゲインを測る。

使い方:
  1. sine50.wav（50Hz, 振幅 0.1, 2秒, 44.1kHz モノ）を用意する
     （生成コードは試験レポート issue-0092 に記載）。
  2. effects 空のプロジェクト dry.json と、eq3 low +12dB の
     プロジェクト eq.json を用意し、それぞれ書き出す:
       daw --export-wav dry.json dry.wav
       daw --export-wav eq.json  eq.wav
  3. python3 measure_eq.py dry.wav eq.wav
後半1秒の左チャンネル RMS 比を dB で出す。
"""
import math
import struct
import sys
import wave


def rms_window(path, from_s, to_s):
    w = wave.open(path)
    rate = w.getframerate()
    ch = w.getnchannels()
    n = w.getnframes()
    data = struct.unpack(f"<{n * ch}h", w.readframes(n))
    left = data[0::ch]
    a, b = int(from_s * rate), int(to_s * rate)
    seg = left[a:b]
    return math.sqrt(sum(x * x for x in seg) / len(seg))


dry = rms_window(sys.argv[1], 1.0, 2.0)
wet = rms_window(sys.argv[2], 1.0, 2.0)
print(f"dry RMS={dry:.1f} wet RMS={wet:.1f} gain={20 * math.log10(wet / dry):+.3f} dB")
