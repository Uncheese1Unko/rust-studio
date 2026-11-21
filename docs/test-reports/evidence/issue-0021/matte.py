#!/usr/bin/env python3
"""Issue #21: run candidate matting models on sample frames, save evidence.

Usage: matte.py <model> <frame.png> <out_prefix>
  model: rvm | modnet | birefnet
Outputs: <out_prefix>_mask.png, <out_prefix>_composite.png (person on green),
and prints per-frame inference time (ms, median of repeats).
"""
import sys, time
import numpy as np
import onnxruntime as ort
from PIL import Image

BASE = "/home/hatch/cutout"
MODELS = {
    "rvm": f"{BASE}/rvm.onnx",
    "modnet": f"{BASE}/modnet.onnx",
    "birefnet": f"{BASE}/hf/onnx/model_fp16.onnx",
}


def load_rgb(path, size=None):
    im = Image.open(path).convert("RGB")
    if size:
        im = im.resize(size, Image.BILINEAR)
    return im


def run_rvm(sess, im):
    # RVM mobilenetv3: input 'src' [1,3,H,W] in [0,1], recurrent states
    # r1..r4 (zero-init for a single frame), 'downsample_ratio'.
    # Outputs 'fgr', 'pha'. Work at 512x288, then upsample the alpha.
    work = im.resize((512, 288), Image.BILINEAR)
    src = np.asarray(work, dtype=np.float32).transpose(2, 0, 1)[None] / 255.0
    inputs = {
        "src": src,
        # 状態の空間サイズは (H*ratio, W*ratio) を /2 /4 /8 /16 したもの
        "r1i": np.zeros((1, 16, 36, 64), dtype=np.float32),
        "r2i": np.zeros((1, 20, 18, 32), dtype=np.float32),
        "r3i": np.zeros((1, 40, 9, 16), dtype=np.float32),
        "r4i": np.zeros((1, 64, 5, 8), dtype=np.float32),
        "downsample_ratio": np.array([0.25], dtype=np.float32),
    }
    names = [o.name for o in sess.get_outputs()]
    d = dict(zip(names, sess.run(None, inputs)))
    pha = d["pha"][0, 0]
    alpha = np.asarray(
        Image.fromarray((pha * 255).astype(np.uint8)).resize(im.size, Image.BILINEAR),
        dtype=np.float32,
    ) / 255.0
    return alpha


def run_modnet(sess, im):
    # MODNet: input 'input' [1,3,512,512] normalized to [-1,1]; output alpha.
    im512 = im.resize((512, 512), Image.BILINEAR)
    x = np.asarray(im512, dtype=np.float32).transpose(2, 0, 1)[None] / 255.0
    x = (x - 0.5) / 0.5
    name = sess.get_inputs()[0].name
    out = sess.run(None, {name: x})[0][0, 0]
    alpha = np.asarray(Image.fromarray((out * 255).astype(np.uint8)).resize(im.size, Image.BILINEAR), dtype=np.float32) / 255.0
    return alpha


def run_birefnet(sess, im):
    # BiRefNet: input [1,3,1024,1024] in [0,1] with ImageNet normalization; output sigmoid alpha.
    im1024 = im.resize((1024, 1024), Image.BILINEAR)
    x = np.asarray(im1024, dtype=np.float32).transpose(2, 0, 1)[None] / 255.0
    mean = np.array([0.485, 0.456, 0.406], dtype=np.float32)[None, :, None, None]
    std = np.array([0.229, 0.224, 0.225], dtype=np.float32)[None, :, None, None]
    x = (x - mean) / std
    name = sess.get_inputs()[0].name
    out = sess.run(None, {name: x.astype(np.float16) if sess.get_inputs()[0].type == 'tensor(float16)' else x})[0][0, 0]
    alpha = np.asarray(Image.fromarray((out * 255).astype(np.uint8)).resize(im.size, Image.BILINEAR), dtype=np.float32) / 255.0
    return alpha


RUNNERS = {"rvm": run_rvm, "modnet": run_modnet, "birefnet": run_birefnet}


def main():
    model, frame_path, prefix = sys.argv[1], sys.argv[2], sys.argv[3]
    sess = ort.InferenceSession(MODELS[model], providers=["CPUExecutionProvider"])
    im = load_rgb(frame_path)
    runner = RUNNERS[model]
    # warmup + timing (3 runs)
    times = []
    alpha = None
    for _ in range(3):
        t0 = time.perf_counter()
        alpha = runner(sess, im)
        times.append((time.perf_counter() - t0) * 1000)
    alpha = np.clip(alpha, 0, 1)
    Image.fromarray((alpha * 255).astype(np.uint8)).save(f"{prefix}_mask.png")
    rgb = np.asarray(im, dtype=np.float32)
    green = np.zeros_like(rgb); green[:] = (0, 200, 0)
    comp = rgb * alpha[..., None] + green * (1 - alpha[..., None])
    Image.fromarray(comp.astype(np.uint8)).save(f"{prefix}_composite.png")
    fg_ratio = float((alpha > 0.5).mean())
    print(f"{model}: median {sorted(times)[1]:.0f} ms/frame (runs: {[f'{t:.0f}' for t in times]}), fg_ratio={fg_ratio:.3f}")


if __name__ == "__main__":
    main()
