# Issue #93 の CLI 測定（daw --export-wav 経由）。
# 前提: インパルス WAV（先頭サンプルだけ 32767、44.1kHz mono 2秒）を
# imp.wav として置き、delay.json / reverb.json（本フォルダ）の
# source がそれを指す。プロジェクトは手書き JSON で、トラックに
# effects を直接書いている（コマンド経由の操作確認は #95）。
# 実行: daw --export-wav delay.json delay.wav / reverb.json も同様。
import struct

SR = 44100


def read_left(path):
    d = open(path, "rb").read()
    n = struct.unpack("<I", d[40:44])[0]
    samples = [struct.unpack_from("<h", d, 44 + i * 2)[0] for i in range(n // 2)]
    return samples[0::2]  # stereo interleaved -> left


def at(samples, ms):
    return samples[int(SR * ms / 1000)] / 32768


def peak(samples, a_ms, b_ms):
    seg = samples[int(SR * a_ms / 1000) : int(SR * b_ms / 1000)]
    return max(abs(x) for x in seg) / 32768


d = read_left("delay.wav")
print(
    "DELAY dry@0: %.4f echo@200ms: %.4f echo@400ms: %.4f echo@600ms: %.4f @100ms: %d"
    % (at(d, 0), at(d, 200), at(d, 400), at(d, 600), at(d, 100) * 32768)
)
r = read_left("reverb.wav")
print(
    "REVERB peaks: [100-300ms] %.4f [300-600ms] %.4f [600-1000ms] %.4f "
    "[900-1000ms] %.5f [1500-2000ms] %.5f"
    % (peak(r, 100, 300), peak(r, 300, 600), peak(r, 600, 1000),
       peak(r, 900, 1000), peak(r, 1500, 2000))
)
# 実測（2026-11-25）:
# DELAY dry@0: 0.8000 echo@200ms: 0.4000 echo@400ms: 0.2000 echo@600ms: 0.1000 @100ms: 0
# REVERB peaks: [100-300ms] 0.0794 [300-600ms] 0.0192 [600-1000ms] 0.0052
#               [900-1000ms] 0.00317 [1500-2000ms] 0.00052
