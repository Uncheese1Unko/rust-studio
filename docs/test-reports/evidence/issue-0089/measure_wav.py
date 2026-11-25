#!/usr/bin/env python3
"""Issue #89 の検証: 取り込み→書き出し WAV のピークと周波数測定。
Goertzel で候補周波数の大きさを測る（純粋 Python、依存なし）。
使い方: measure_wav.py <wav>"""
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


def main():
    w = wave.open(sys.argv[1])
    n, rate, ch = w.getnframes(), w.getframerate(), w.getnchannels()
    raw = struct.unpack(f"<{n * ch}h", w.readframes(n))
    left = [raw[i * ch] / 32768.0 for i in range(n)]
    peak = max(abs(s) for s in raw)
    print(f"frames={n} rate={rate} ch={ch} duration={n / rate:.2f}s peak={peak}")
    assert peak > 0, "FAIL: silent"

    def seg(a_ms, b_ms):
        return left[int(a_ms * rate / 1000) : int(b_ms * rate / 1000)]

    # Lead: 69 (A4 440Hz) 0-400ms / 72 (C5 523.25Hz) 400-800ms /
    #       76 (E5 659.25Hz) 800-1600ms
    # Bass: 45 (A2 110Hz) 0-800ms / 41 (F2 87.31Hz) 800-1600ms
    checks = [
        ("0-400ms   Lead A4 440.00", seg(50, 380), 440.00),
        ("0-400ms   Bass A2 110.00", seg(50, 380), 110.00),
        ("400-800ms Lead C5 523.25", seg(450, 780), 523.25),
        ("800-1500ms Lead E5 659.25", seg(850, 1480), 659.25),
        ("800-1500ms Bass F2 87.31", seg(850, 1480), 87.31),
    ]
    for label, samples, freq in checks:
        mag = goertzel(samples, rate, freq)
        off = goertzel(samples, rate, freq * 1.5)
        print(f"{label}: goertzel={mag:.4f} (x1.5 reference={off:.4f})")
        assert mag > 0.03, f"FAIL: {label} not present"
        assert mag > off, f"FAIL: {label} not dominant over x1.5"
    print("MEASURE OK")


if __name__ == "__main__":
    main()
