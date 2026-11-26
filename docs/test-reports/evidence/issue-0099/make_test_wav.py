import math, struct, os

sr = 44100
notes = [(69, 0.0, 0.5), (72, 0.5, 0.5), (76, 1.0, 0.5), (81, 1.5, 0.5)]
def freq(m): return 440.0 * 2 ** ((m - 69) / 12)

n = sr * 2
mel = [0.0] * n
for m, start, dur in notes:
    f = freq(m)
    s0 = int(start * sr)
    s1 = int((start + dur) * sr)
    for i in range(s0, s1):
        tt = (i - s0) / sr
        env = min(tt / 0.02, 1.0) * math.exp(-tt * 1.5)
        mel[i] += env * (math.sin(2*math.pi*f*tt) + 0.5*math.sin(2*math.pi*2*f*tt) + 0.25*math.sin(2*math.pi*3*f*tt))
acc = [0.0] * n
for m in (48, 52, 55):
    f = freq(m)
    for i in range(n):
        t = i / sr
        acc[i] += 0.3 * math.sin(2*math.pi*f*t) * (0.8 + 0.2*math.sin(2*math.pi*0.5*t))
mix = [0.6*mel[i] + 0.4*acc[i] for i in range(n)]
peak = max(abs(x) for x in mix)
mix = [x / peak * 0.9 for x in mix]

def write_wav(path, samples):
    with open(path, 'wb') as fh:
        data = b''.join(struct.pack('<hh', int(max(-1.0, min(1.0, s)) * 32767), int(max(-1.0, min(1.0, s)) * 32767)) for s in samples)
        fh.write(b'RIFF' + struct.pack('<I', 36 + len(data)) + b'WAVE')
        fh.write(b'fmt ' + struct.pack('<IHHIIHH', 16, 1, 2, sr, sr*4, 4, 16))
        fh.write(b'data' + struct.pack('<I', len(data)) + data)

base = os.path.expanduser('~/demucs-work')
write_wav(os.path.join(base, 'input.wav'), mix)
write_wav(os.path.join(base, 'melody_only.wav'), [0.6*x for x in mel])
print('wrote input.wav / melody_only.wav, notes:', [(m, round(freq(m),2)) for m,_,_ in notes])
