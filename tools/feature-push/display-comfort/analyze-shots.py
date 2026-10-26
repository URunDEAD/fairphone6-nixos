#!/usr/bin/env python3
"""Compare DRM framebuffer screenshots of the display-comfort live test.

  analyze-shots.py BASE.png FILTERED.png KELVIN GREY [--box x0,y0,x1,y1]

The framebuffer holds what the panel shows (the filter is drawn by the
shell), so FILTERED should equal M * BASE per pixel, M = diag(white point of
KELVIN) * ((1-GREY) I + GREY L). Prints the mean absolute error per channel
in 8-bit steps over pixels that are identical in content (only static parts
of the screen are compared: pixels whose BASE value is far from saturation
issues are all used; animated regions should be excluded with --box).
"""
import math
import sys

from PIL import Image


def white_point(k):
    def raw(k):
        t = k / 100.0
        if t <= 66:
            r = 255.0
            g = 99.4708025861 * math.log(t) - 161.1195681661
        else:
            r = 329.698727446 * (t - 60) ** -0.1332047592
            g = 288.1221695283 * (t - 60) ** -0.0755148492
        if t >= 66:
            b = 255.0
        elif t <= 19:
            b = 0.0
        else:
            b = 138.5177312231 * math.log(t - 10) - 305.0447927307
        return [min(255.0, max(0.0, x)) for x in (r, g, b)]
    w, c = raw(6500), raw(k)
    return [min(1.0, c[i] / w[i]) for i in range(3)]


def matrix(k, grey):
    rgb = white_point(k)
    lum = (0.2126, 0.7152, 0.0722)
    return [[rgb[r] * ((1 - grey if r == c else 0) + grey * lum[c]) for c in range(3)] for r in range(3)]


def main():
    base, filt, k, grey = sys.argv[1], sys.argv[2], int(sys.argv[3]), float(sys.argv[4])
    box = None
    if '--box' in sys.argv:
        box = tuple(int(v) for v in sys.argv[sys.argv.index('--box') + 1].split(','))
    a = Image.open(base).convert('RGB')
    b = Image.open(filt).convert('RGB')
    if box:
        a, b = a.crop(box), b.crop(box)
    m = matrix(k, grey)
    pa, pb = a.load(), b.load()
    w, h = a.size
    err = [0.0, 0.0, 0.0]
    n = 0
    worst = 0
    for y in range(0, h, 3):
        for x in range(0, w, 3):
            s = pa[x, y]
            o = pb[x, y]
            for r in range(3):
                e = sum(m[r][c] * s[c] for c in range(3))
                d = abs(min(255, max(0, e)) - o[r])
                err[r] += d
                worst = max(worst, d)
            n += 1
    print(f"expected matrix rows: {[[round(v, 3) for v in row] for row in m]}")
    print(f"mean |error| R {err[0]/n:.2f}  G {err[1]/n:.2f}  B {err[2]/n:.2f} (8-bit steps, {n} samples), max {worst:.0f}")
    grey_px = sum(1 for y in range(0, h, 9) for x in range(0, w, 9)
                  if max(pb[x, y]) - min(pb[x, y]) <= 3)
    tot = len(range(0, h, 9)) * len(range(0, w, 9))
    print(f"neutral (R=G=B within 3) pixels in FILTERED: {100.0 * grey_px / tot:.1f} %")


if __name__ == '__main__':
    main()
