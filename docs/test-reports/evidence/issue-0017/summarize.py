import json, sys
E = sys.argv[1]
r = json.load(open(f"{E}/measurements.json", encoding="utf-8"))
M = ["naive_resample_atempo","rubberband_formant_shifted","rubberband_formant_preserved","rubberband_formant_preserved_pitchq_quality","signalsmith_formant_shifted","signalsmith_formant_preserved"]
H = ["基準: リサンプル+WSOLA","RB 保持なし","RB 保持あり","RB 保持あり+pitchq=quality","SS 補正なし","SS 補正あり"]
out = []
out.append("フォルマント移動率 α（1.00 = 包絡が動いていない＝保持。ピッチと一緒に動くとシフト比に等しい）\n")
out.append("| 素材 | シフト | 目標比 | " + " | ".join(H) + " |"); out.append("|" + "---|" * (3 + len(H)))
for s in sorted({x['src'] for x in r}):
    for st in (4, 7, 12, -4, -7):
        d = {x['method']: x for x in r if x['src'] == s and x['semitones'] == st}
        out.append(f"| {s} | {st:+d} | {2**(st/12):.2f} | " + " | ".join(f"{d[m]['formant_alpha']:.2f}" for m in M) + " |")
out.append("\n声3本の |α-1| 平均（小さいほど保持）\n")
out.append("| 方式 | 上げ(+4,+7,+12) | 下げ(-4,-7) |"); out.append("|---|---|---|")
for m, h in zip(M, H):
    v = [x for x in r if x['method'] == m and x['src'].startswith('voice')]
    f = lambda g: (lambda e: sum(e)/len(e))([abs(x['formant_alpha']-1) for x in v if g(x)])
    out.append(f"| {h} | {f(lambda x: x['semitones']>0):.2f} | {f(lambda x: x['semitones']<0):.2f} |")
open(f"{E}/measurements.md", "w", encoding="utf-8").write("\n".join(out) + "\n")
