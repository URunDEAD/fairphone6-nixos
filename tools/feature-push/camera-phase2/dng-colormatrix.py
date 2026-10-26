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


# camera phase 2: the same CamX data of the other two sensors, read from
# com.qti.tuned.tsp_ov13b10_fp6.bin / com.qti.tuned.tsp_s5kkd1_fp6.bin
# (stock vendor_a.img, read with debugfs, never mounted): bright-zone
# cc13 CCMs (CCT regions 2750-2950 / 3900-4100 / 4900-5100 / 6400-6600
# for the OV13B10; 1-2500 / 2700-3400 / 3800-4300 / 4800-10000 for the
# S5KKD1) and the 10-point AWB reference gains (same layout and labels as
# the IMX896 table: 9500 7504 6504 5000 F F F 2856 2300 1900 K).
OV13B10_LOCUS = [(1900, 0.6646, 3.2713), (2300, 0.8589, 2.5183), (2856, 1.0751, 2.1743),
                 (5000, 1.8262, 1.5663), (6504, 1.9992, 1.4252), (7504, 1.9652, 1.1881),
                 (9500, 2.727, 0.9697)]
OV13B10_CCM = {2850: [1.7705, -0.9246, 0.1541, -0.1767, 1.1485, 0.0281, -0.1775, -1.0638, 2.2413],
               4000: [1.7245, -0.7111, -0.0134, -0.2077, 1.5454, -0.3378, -0.2252, -1.2324, 2.4576],
               5000: [1.8546, -0.7611, -0.0935, -0.1763, 1.4713, -0.2950, -0.1424, -0.8835, 2.0259],
               6500: [1.8626, -0.7911, -0.0715, -0.1593, 1.5187, -0.3595, -0.1084, -0.8925, 2.0010]}
S5KKD1_LOCUS = [(1900, 0.745, 2.4732), (2300, 0.906, 2.2529), (2856, 1.0641, 2.1229),
                (5000, 1.4811, 1.5649), (6504, 1.6334, 1.4425), (7504, 1.7084, 1.3349),
                (9500, 2.5218, 1.0966)]
S5KKD1_CCM = {2300: [2.0893, -1.1265, 0.0372, -0.1863, 1.1897, -0.0035, -0.2248, -0.9986, 2.2235],
              3050: [1.9608, -0.9711, 0.0104, -0.2481, 1.3030, -0.0549, -0.2083, -0.8267, 2.0350],
              4000: [2.2459, -1.2690, 0.0231, -0.2362, 1.3365, -0.1003, -0.1452, -0.7450, 1.8903],
              6500: [2.1181, -0.9128, -0.2052, -0.2871, 1.6240, -0.3369, -0.0970, -0.7710, 1.8680]}


if __name__ == "__main__":
    emit("imx896", [("A", IMX896_CCM[2800], interp_gains(IMX896_LOCUS, 2856)),
                    ("D65", IMX896_CCM[5750], interp_gains(IMX896_LOCUS, 6504))])
    emit("ov13b10", [("A", OV13B10_CCM[2850], interp_gains(OV13B10_LOCUS, 2856)),
                     ("D65", OV13B10_CCM[6500], interp_gains(OV13B10_LOCUS, 6504))])
    emit("s5kkd1", [("A", S5KKD1_CCM[3050], interp_gains(S5KKD1_LOCUS, 2856)),
                    ("D65", S5KKD1_CCM[6500], interp_gains(S5KKD1_LOCUS, 6504))])
