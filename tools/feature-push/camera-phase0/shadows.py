#!/usr/bin/env python3
"""Channel means per luma band of an analysed preview/centre JPEG or raw RGB.
usage: shadows.py IMAGE.jpg"""
import sys
import numpy as np
from PIL import Image
a = np.asarray(Image.open(sys.argv[1]).convert("RGB")).astype(np.float32)
Y = a @ np.array([0.299, 0.587, 0.114])
for lo, hi in [(0, 15), (15, 30), (30, 60), (60, 100), (100, 160), (160, 230), (230, 256)]:
    m = (Y >= lo) & (Y < hi)
    if m.sum() < 50:
        continue
    r, g, b = a[m].mean(axis=0)
    print(f"Y {lo:3d}-{hi:3d} n={m.mean():.3f} rgb=({r:6.1f},{g:6.1f},{b:6.1f}) g/r={g/max(r,.1):.2f} g/b={g/max(b,.1):.2f}")
