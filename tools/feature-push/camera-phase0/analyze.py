#!/usr/bin/env python3
"""Phase 0 frame metrics: channel means (whole frame, centre, near-neutral
pixels), clipping, and a centre sharpness metric (Tenengrad-like, mean of
squared gradients on luma, normalised by luma mean). Writes a PNG preview
(downscaled) and a full-res centre crop.
usage: analyze.py RAW.rgb WIDTH HEIGHT OUTPREFIX [label]"""
import sys, json
import numpy as np
from PIL import Image

raw, w, h, out = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
label = sys.argv[5] if len(sys.argv) > 5 else out
a = np.fromfile(raw, dtype=np.uint8)[: w * h * 3].reshape(h, w, 3).astype(np.float32)
Y = 0.299 * a[..., 0] + 0.587 * a[..., 1] + 0.114 * a[..., 2]
ch, cw = h // 4, w // 4
c = a[h // 2 - ch // 2: h // 2 + ch // 2, w // 2 - cw // 2: w // 2 + cw // 2]
cy = Y[h // 2 - ch // 2: h // 2 + ch // 2, w // 2 - cw // 2: w // 2 + cw // 2]
gx = np.diff(cy, axis=1)[:-1, :]
gy = np.diff(cy, axis=0)[:, :-1]
sharp = float(np.mean(gx * gx + gy * gy) / max(cy.mean(), 1.0))
# near-neutral pixels: mid-tones with low chroma after normalising by luma
mid = (Y > 40) & (Y < 220)
mx = a.max(axis=2); mn = a.min(axis=2)
neutral = mid & ((mx - mn) < 0.25 * Y + 6)
def means(x):
    return [round(float(v), 1) for v in x.reshape(-1, 3).mean(axis=0)]
res = {
    "label": label,
    "mean_rgb": means(a),
    "centre_rgb": means(c),
    "neutral_frac": round(float(neutral.mean()), 3),
    "neutral_rgb": means(a[neutral]) if neutral.any() else None,
    "luma_mean": round(float(Y.mean()), 1),
    "clip_hi": round(float((a >= 250).any(axis=2).mean()), 4),
    "clip_lo": round(float((a <= 3).all(axis=2).mean()), 4),
    "centre_sharpness": round(sharp, 2),
}
m = res["mean_rgb"]
res["g_over_r"] = round(m[1] / max(m[0], 1e-3), 3)
res["g_over_b"] = round(m[1] / max(m[2], 1e-3), 3)
print(json.dumps(res))
img = Image.fromarray(a.astype(np.uint8))
prev = img.copy(); prev.thumbnail((800, 800)); prev.save(out + "-preview.jpg", quality=88)
Image.fromarray(c.astype(np.uint8)).save(out + "-centre.jpg", quality=92)
with open(out + ".json", "w") as f:
    json.dump(res, f, indent=1)
