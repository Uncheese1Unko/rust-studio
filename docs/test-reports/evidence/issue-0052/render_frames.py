#!/usr/bin/env python3
"""Issue #52 試作用の使い捨てスクリプト（本体コードではない）。

タイムライン（映像2クリップ＋境界に1秒のクロスディゾルブ）を
30fps の PNG フレーム列にレンダリングする。合成規則は ADR-0014 の
提案方式（窓は後続クリップの先頭、α は直線、前クリップはソースの
続きのフレームを使う）。

前提: fa/ と fb/ に素材の全フレーム PNG（30fps）が展開済みであること。
使い方: python3 render_frames.py  # frames/ に150枚を書く
"""
from PIL import Image
import os

FPS = 30
DUR_MS = 5000
A_START, A_END = 0, 2000
B_START = 2000
TRANS_MS = 1000  # B の transition_in


def frame_path(d, src_ms):
    idx = int(src_ms * FPS // 1000)
    files = sorted(os.listdir(d))
    idx = min(idx, len(files) - 1)
    return os.path.join(d, files[idx])


def main():
    n = DUR_MS * FPS // 1000
    for k in range(n):
        t = k * 1000 // FPS
        if t < A_END:
            img = Image.open(frame_path('fa', t - A_START)).convert('RGB')
        elif t < B_START + TRANS_MS:
            alpha = (t - B_START) / TRANS_MS
            fa = Image.open(frame_path('fa', t - A_START)).convert('RGB')
            fb = Image.open(frame_path('fb', t - B_START)).convert('RGB')
            img = Image.blend(fa, fb, alpha)
        else:
            img = Image.open(frame_path('fb', t - B_START)).convert('RGB')
        img.save(f'frames/f{k:03d}.png')
    print('frames rendered:', n)


if __name__ == '__main__':
    main()
