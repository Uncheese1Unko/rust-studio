import sys, os, glob, json
import numpy as np
from scipy.io import wavfile
from scipy.signal import get_window

N, H, SR = 2048, 512, 44100
win = get_window("hann", N)
LIFTER = 50  # ケプストラム低次 → スペクトル包絡

def load(p):
    sr, x = wavfile.read(p); assert sr == SR
    return x.astype(np.float64) / 32768.0

def frames(x):
    n = 1 + max(0, (len(x) - N) // H)
    return np.stack([x[i*H:i*H+N] * win for i in range(n)])

def envelope_and_f0(x, thr_ratio=0.1):
    F = frames(x)
    rms = np.sqrt((F**2).mean(1)); keep = rms > thr_ratio * rms.max()
    envs, f0s = [], []
    for fr in F[keep]:
        mag = np.abs(np.fft.rfft(fr)) + 1e-9
        c = np.fft.irfft(np.log(mag))
        lift = np.zeros_like(c); lift[:LIFTER] = 1; lift[-LIFTER+1:] = 1
        envs.append(np.fft.rfft(c * lift).real)          # 対数包絡
        # F0: 自己相関 (60–1000Hz)
        a = np.correlate(fr, fr, "full")[N-1:]
        lo, hi = SR // 1000, SR // 60
        if a[0] > 0:
            k = lo + np.argmax(a[lo:hi]);
            if a[k] / a[0] > 0.5: f0s.append(SR / k)
    return np.mean(envs, 0), (np.median(f0s) if f0s else float("nan"))

freqs = np.fft.rfftfreq(N, 1 / SR)
band = (freqs > 150) & (freqs < 5000)

def warp_alpha(env_ref, env_out):
    """env_out(f) ≈ env_ref(f/α) となる α を探す（α>1 でフォルマントが上へ移動）"""
    best, ba = 1e18, 1.0
    for a in np.exp(np.linspace(np.log(0.5), np.log(2.4), 400)):
        ref_w = np.interp(freqs[band] / a, freqs, env_ref)
        e = np.mean(((env_out[band] - env_out[band].mean()) - (ref_w - ref_w.mean()))**2)
        if e < best: best, ba = e, a
    return ba

def main(E):
    rows = []
    for src in sorted(glob.glob(f"{E}/src/*.wav")):
        n = os.path.basename(src)[:-4]
        eo, fo = envelope_and_f0(load(src))
        for st in (4, 7, 12, -4, -7):
            tgt = 2 ** (st / 12)
            for m in ("naive_resample_atempo", "rubberband_formant_shifted", "rubberband_formant_preserved",
                      "signalsmith_formant_shifted", "signalsmith_formant_preserved","rubberband_formant_preserved_pitchq_quality"):
                p = f"{E}/{m}/{n}_{st:+d}st.wav"
                e, f = envelope_and_f0(load(p))
                rows.append(dict(src=n, semitones=st, method=m, target=round(tgt, 3),
                                 f0_ratio=round(f / fo, 3), formant_alpha=round(warp_alpha(eo, e), 3)))
    json.dump(rows, open(f"{E}/measurements.json", "w"), indent=1, ensure_ascii=False)
    # 表
    print("src | st | " + " | ".join(m.replace('_formant','').replace('naive_resample_atempo','naive') for m in
          ("naive_resample_atempo","rubberband_formant_shifted","rubberband_formant_preserved","rubberband_formant_preserved_pitchq_quality","signalsmith_formant_shifted","signalsmith_formant_preserved")))
    for src in sorted({r['src'] for r in rows}):
        for st in (4, 7, 12, -4, -7):
            sel = {r['method']: r for r in rows if r['src']==src and r['semitones']==st}
            print(f"{src} | {st:+d} (x{2**(st/12):.2f}) | " + " | ".join(
                f"F0 x{sel[m]['f0_ratio']:.2f} / α {sel[m]['formant_alpha']:.2f}" for m in sel))
main(sys.argv[1])
