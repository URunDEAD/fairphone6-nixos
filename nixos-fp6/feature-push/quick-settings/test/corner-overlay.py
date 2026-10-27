#!/usr/bin/env python3
"""Draw the FP6 rounded-corner safe area on a 1116x2484 canvas (and, if a
status-bar crop of a DRM screenshot is given, over it).
  corner-overlay.py OUT.png [RADIUS_PX] [statusbar-crop.png OUT2.png]"""
import math, sys
from PIL import Image, ImageDraw, ImageFont

W, H, GU = 1116, 2484, 24
r = int(sys.argv[2]) if len(sys.argv) > 2 else 110

def inset(top):
    if top >= r: return 0
    dy = r - top
    return math.ceil(r - math.sqrt(r * r - dy * dy))

bar, ebar = 3 * GU, 7 * GU
ins_c, ins_e = inset((bar - 2 * GU) / 2), inset(1 * GU)
gap = 4 * GU
img = Image.new("RGB", (W, H), "#202020")
d = ImageDraw.Draw(img)
try:
    font = ImageFont.truetype("DejaVuSans.ttf", 30)
except OSError:
    font = ImageFont.load_default()

# display mask: everything outside the rounded rectangle is not visible
mask = Image.new("L", (W, H), 0)
ImageDraw.Draw(mask).rounded_rectangle([0, 0, W - 1, H - 1], radius=r, fill=255)
img.paste(Image.new("RGB", (W, H), "#3a3a3a"), mask=mask)
d.rectangle([0, 0, W, bar], fill="#000000")
img.paste(Image.new("RGB", (W, H), "#000000"), mask=Image.eval(mask, lambda v: 255 - v))
# collapsed status bar safe area
d.rectangle([ins_c, GU // 2, W - ins_c, bar - GU // 2], outline="#34c759", width=3)
# expanded row safe area
d.rectangle([ins_e, bar + 40, W - ins_e, bar + 40 + ebar], outline="#30b0ff", width=3)
d.text((ins_e + 10, bar + 50), f"expanded row: inset {ins_e}px", fill="#30b0ff", font=font)
# camera hole gap (lomiri-polish cameraCutoutGap = 4 gu)
d.rectangle([(W - gap) // 2, 0, (W + gap) // 2, bar], outline="#ff9f0a", width=3)
d.ellipse([W // 2 - 26, bar // 2 - 26, W // 2 + 26, bar // 2 + 26], outline="#ff9f0a", width=3)
# arcs
for cx, cy in [(r, r), (W - r, r), (r, H - r), (W - r, H - r)]:
    d.ellipse([cx - r, cy - r, cx + r, cy + r], outline="#ff3b30", width=2)
y = 380
for line, col in [(f"corner radius r = {r} px (estimate, configurable)", "#ff3b30"),
                  (f"status bar 72 px, icons 48 px high from y=12: inset {ins_c} px", "#34c759"),
                  (f"expanded row content from y=24: inset {ins_e} px", "#30b0ff"),
                  ("camera hole gap 96 px (lomiri-polish)", "#ff9f0a"),
                  ("inset(y) = r - sqrt(r^2 - (r - y)^2)", "#dddddd"),
                  (f"bottom: launcher icons start >= {math.ceil(r - math.sqrt(r*r - (r - 30)**2))} px", "#dddddd"),
                  ("above the edge (already 36 px: no change)", "#dddddd")]:
    d.text((60, y), line, fill=col, font=font); y += 48
img.save(sys.argv[1])
print(f"r={r} collapsed inset={ins_c} expanded inset={ins_e}")

if len(sys.argv) > 4:
    crop = Image.open(sys.argv[3]).convert("RGB")
    cw, ch = crop.size
    out = Image.new("RGB", (cw, ch * 2 + 20), "#606060")
    m = Image.new("L", (cw, ch), 0)
    ImageDraw.Draw(m).rounded_rectangle([0, 0, cw - 1, H], radius=r, fill=255)
    vis = Image.new("RGB", (cw, ch), "#606060"); vis.paste(crop, mask=m)
    out.paste(vis, (0, 0))
    dd = ImageDraw.Draw(out)
    dd.rectangle([ins_c, 12, cw - ins_c, 60], outline="#34c759", width=2)
    dd.text((10, ch + 2), "above: today's bar seen through the rounded corners; green = safe area", fill="white", font=font)
    out.save(sys.argv[4])
