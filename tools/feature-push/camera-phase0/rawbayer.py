#!/usr/bin/env python3
"""Render a CSI-2 packed 10-bit Bayer frame (as written by `cam -s role=raw`)
in a given Bayer order: 2x2 binning, black level 64, gray-world white
balance, sRGB gamma. Used to decide a sensor's real CFA order objectively.
usage: rawbayer.py RAW WIDTH HEIGHT STRIDE ORDER(RGGB|GRBG|GBRG|BGGR) OUT.jpg"""
import sys
import numpy as np
from PIL import Image

raw, w, h, stride, order, out = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4]), sys.argv[5].upper(), sys.argv[6]
b = np.fromfile(raw, dtype=np.uint8)[: stride * h].reshape(h, stride)[:, : w * 5 // 4]
b = b.reshape(h, w // 4, 5).astype(np.uint16)
px = np.empty((h, w // 4, 4), np.uint16)
for i in range(4):
    px[:, :, i] = (b[:, :, i] << 2) | ((b[:, :, 4] >> (2 * i)) & 3)
img = px.reshape(h, w).astype(np.float32) - 64.0
pos = {order[0]: [(0, 0)], order[3]: [(1, 1)]}
pos.setdefault("G", [])
pos["G"] = [(0, 1), (1, 0)] if order[0] in "RB" else [(0, 0), (1, 1)]
pos["R"] = [(y, x) for (y, x), c in zip([(0, 0), (0, 1), (1, 0), (1, 1)], order) if c == "R"]
pos["B"] = [(y, x) for (y, x), c in zip([(0, 0), (0, 1), (1, 0), (1, 1)], order) if c == "B"]
ch = []
for c in "RGB":
    ch.append(np.mean([img[y::2, x::2] for (y, x) in pos[c]], axis=0))
rgb = np.stack(ch, axis=-1).clip(0, None)
m = rgb.reshape(-1, 3).mean(axis=0)
rgb *= m[1] / m
rgb /= np.percentile(rgb, 99.5)
rgb = rgb.clip(0, 1)
enc = np.where(rgb <= 0.0031308, 12.92 * rgb, 1.055 * rgb ** (1 / 2.4) - 0.055)
im = Image.fromarray((enc * 255).astype(np.uint8))
im.thumbnail((800, 800))
im.save(out, quality=90)
print(order, "gray-world gains", [round(float(m[1] / v), 3) for v in m])
