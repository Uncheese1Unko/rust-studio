#!/usr/bin/env python3
"""Issue #89 の回帰測定: サンプラーへ差し替えた書き出し WAV が、
サンプラーの音高（369.99Hz / 440.00Hz）で鳴り、シンセの音高
（440.00Hz / 523.25Hz）が消えていること。
使い方: measure_replace.py <wav>"""
import math
import struct
import sys
import wave


def goertzel(samples, rate, freq):
    n = len(samples)
    k = 2.0 * math.pi * freq / rate
    coeff = 2.0 * math.cos(k)
    s1 = s2 = 0.0
    for x in samples:
        s = x + coeff * s1 - s2
        s2 = s1
        s1 = s
    return math.sqrt(s1 * s1 + s2 * s2 - coeff * s1 * s2) / n


w = wave.open(sys.argv[1])
n, rate, ch = w.getnframes(), w.getframerate(), w.getnchannels()
raw = struct.unpack(f"<{n * ch}h", w.readframes(n))
left = [raw[i * ch] / 32768.0 for i in range(n)]
print("peak:", max(abs(s) for s in raw))
seg1 = left[int(0.05 * rate) : int(0.38 * rate)]
seg2 = left[int(0.45 * rate) : int(0.78 * rate)]
g1s = goertzel(seg1, rate, 369.99)
g1y = goertzel(seg1, rate, 440.00)
g2s = goertzel(seg2, rate, 440.00)
g2y = goertzel(seg2, rate, 523.25)
print(f"0-400ms   sampler 369.99Hz: {g1s:.4f} / synth 440Hz: {g1y:.4f}")
print(f"400-800ms sampler 440.00Hz: {g2s:.4f} / synth 523.25Hz: {g2y:.4f}")
assert g1s > 0.03 and g1s > g1y
assert g2s > 0.03 and g2s > g2y
print("REPLACE MEASURE OK")
