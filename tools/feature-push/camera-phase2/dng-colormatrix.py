#!/usr/bin/env python3
"""DNG ColorMatrix from a softISP CCM + AWB gains (camera phase 2).

softISP: srgb_linear = CCM * diag(gR, 1, gB) * raw   (CCM rows sum to 1,
i.e. the illuminant's white maps to sRGB white: an adapted rendition).
DNG:     camera_raw  = ColorMatrix * XYZ(illuminant, not adapted).
=> ColorMatrix = diag(1/g) * inv(CCM) * M_XYZ->sRGB(D65) * Bradford(L->D65),
normalised so that the illuminant white (Y = 1) maps to max channel 1.
usage: dng-colormatrix.py  (prints C tables for fp6camera-raw.c)"""
import numpy as np

XYZ2SRGB = np.array([[3.2404, -1.5371, -0.4985],
                     [-0.9692, 1.8760, 0.0415],
                     [0.0556, -0.2040, 1.0572]])
BRADFORD = np.array([[0.8951, 0.2664, -0.1614],
                     [-0.7502, 1.7135, 0.0367],
                     [0.0389, -0.0685, 1.0296]])
WHITE = {"A": np.array([1.0985, 1.0, 0.3558]),
         "D65": np.array([0.9504, 1.0, 1.0888]),
         "D50": np.array([0.9642, 1.0, 0.8252])}
EXIF_ILLUMINANT = {"A": 17, "D65": 21, "D50": 23}


def adapt(src, dst):
    s = BRADFORD @ WHITE[src]
    d = BRADFORD @ WHITE[dst]
    return np.linalg.inv(BRADFORD) @ np.diag(d / s) @ BRADFORD


def colour_matrix(ccm, gr, gb, illum):
    ccm = np.array(ccm, dtype=float).reshape(3, 3)
    m = np.diag([1 / gr, 1.0, 1 / gb]) @ np.linalg.inv(ccm) @ XYZ2SRGB @ adapt(illum, "D65")
    neutral = m @ WHITE[illum]
    return m / neutral.max()


def interp_gains(locus, ct):
    locus = sorted(locus)
    for (c0, r0, b0), (c1, r1, b1) in zip(locus, locus[1:]):
        if c0 <= ct <= c1:
            t = (1 / ct - 1 / c0) / (1 / c1 - 1 / c0)   # mired interpolation
            return r0 + t * (r1 - r0), b0 + t * (b1 - b0)
    raise ValueError(ct)


IMX896_LOCUS = [(1900, 0.8789, 4.7716), (2300, 1.1191, 3.3405),
                (2856, 1.3779, 2.7694), (5000, 2.0373, 1.8091),
                (6504, 2.3671, 1.5740), (7504, 2.7419, 1.3572),
                (9500, 4.5084, 1.0121)]
IMX896_CCM = {2800: [1.5934, -0.5527, -0.0406, -0.1664, 1.1704, -0.0039,
                     0.0041, -0.8555, 1.8514],
              5750: [1.9000, -0.5812, -0.3187, -0.2925, 1.7781, -0.4856,
                     0.0145, -0.7348, 1.7202]}


def emit(name, pairs):
    print(f"/* {name} */")
    for illum, ccm, gains in pairs:
        m = colour_matrix(ccm, *gains, illum)
        print(f"  /* {illum} (EXIF {EXIF_ILLUMINANT[illum]}), gains R {gains[0]:.4f} B {gains[1]:.4f} */")
        print("  {" + ", ".join(f"{v:.4f}" for v in m.flatten()) + "},")


if __name__ == "__main__":
    emit("imx896", [("A", IMX896_CCM[2800], interp_gains(IMX896_LOCUS, 2856)),
                    ("D65", IMX896_CCM[5750], interp_gains(IMX896_LOCUS, 6504))])
