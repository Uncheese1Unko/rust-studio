import numpy as np, soundfile as sf
sr = 44100
# melody: A4 C5 E5 A5, 0.5s each, sine + 2 harmonics
notes = [(69, 0.0, 0.5), (72, 0.5, 0.5), (76, 1.0, 0.5), (81, 1.5, 0.5)]
def freq(m): return 440.0 * 2 ** ((m - 69) / 12)
t = np.arange(sr * 2) / sr
mel = np.zeros_like(t)
for m, start, dur in notes:
    seg = (t >= start) & (t < start + dur)
    tt = t[seg] - start
    env = np.minimum(tt / 0.02, 1.0) * np.exp(-tt * 1.5)
    f = freq(m)
    mel[seg] += env * (np.sin(2*np.pi*f*tt) + 0.5*np.sin(2*np.pi*2*f*tt) + 0.25*np.sin(2*np.pi*3*f*tt))
# accompaniment: C major pad (C3 E3 G3) whole 2s, soft
acc = np.zeros_like(t)
for m in (48, 52, 55):
    f = freq(m)
    acc += 0.3 * np.sin(2*np.pi*f*t) * (0.8 + 0.2*np.sin(2*np.pi*0.5*t))
mix = 0.6 * mel + 0.4 * acc
mix = mix / np.max(np.abs(mix)) * 0.9
stereo = np.column_stack([mix, mix])
sf.write('/tmp/sp-test/input.wav', stereo, sr)
sf.write('/tmp/sp-test/melody_only.wav', np.column_stack([0.6*mel]*2), sr)
print('wrote input.wav, melody notes:', [(m, freq(m)) for m,_,_ in notes])
