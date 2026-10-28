# Assertions for the UITK font scale offscreen test (see default.nix).
import json, os, sys
from PIL import Image, ImageChops

out, shim = sys.argv[1], sys.argv[2]
r = {n: json.load(open(os.path.join(out, n + ".json")))
     for n in ["stock", "stock-env13", "shim-unset", "shim-100", "shim-130", "shim-085", "shim-bad"]}
lib = {n: open(os.path.join(out, n + ".lib")).read() for n in list(r) + ["app"]}
keys = ["xsmall", "small", "medium", "large", "xlarge", "fontUtilsLarge"]
fails = []

def check(cond, msg):
    print(("PASS " if cond else "FAIL ") + msg)
    if not cond:
        fails.append(msg)

def img(n):
    return Image.open(os.path.join(out, n + ".png")).convert("RGBA")

check(shim not in lib["stock"] and "libLomiriToolkit" in lib["stock"], "stock run maps the stock libLomiriToolkit")
for n in ["shim-unset", "shim-100", "shim-130"]:
    check(shim in lib[n] and lib[n].count("libLomiriToolkit") == 1, n + " maps only the font-scale libLomiriToolkit")
check(shim in lib["app"] and lib["app"].count("libLomiriToolkit") == 1, "wrapped app maps only the font-scale libLomiriToolkit")
lib["app-stock"] = open(os.path.join(out, "app-stock.lib")).read()
check(shim not in lib["app-stock"], "wrapped app without LD_LIBRARY_PATH maps the stock libLomiriToolkit")
rc = {n: open(os.path.join(out, n + ".rc")).read().strip() for n in ["app", "app-stock"]}
check(rc["app"] == rc["app-stock"], "wrapped app ends the same way with and without it (rc %s / %s)" % (rc["app"], rc["app-stock"]))

base = r["stock"]
for n in ["stock-env13", "shim-unset", "shim-100", "shim-bad"]:
    check(all(r[n][k] == base[k] for k in keys) and r[n]["columnHeight"] == base["columnHeight"],
          n + " sizes equal stock " + str([base[k] for k in keys]))
    a, b = img("stock"), img(n)
    check(a.size == b.size and ImageChops.difference(a, b).getbbox() is None, n + " grab pixel-identical to stock")

for n, f in [("shim-130", 1.30), ("shim-085", 0.85)]:
    ok = all(abs(r[n][k] - base[k] * f) <= 1.0 for k in keys)
    check(ok, "%s sizes = stock x %.2f (+-1 px rounding): %s" % (n, f, [r[n][k] for k in keys]))
    ratio = r[n]["columnHeight"] / base["columnHeight"]
    check(abs(ratio - f) < 0.08, "%s text block height ratio %.3f" % (n, ratio))
check(r["shim-130"]["gu"] == base["gu"], "grid unit unchanged by the font scale")

# Bold text: QFont::Light = 25, QFont::Normal = 50 (Qt 5).
rb = {n: json.load(open(os.path.join(out, n + ".json"))) for n in ["stock-bold", "shim-bold", "shim-bold0"]}
check(r["stock"]["mediumWeight"] == 25 and r["shim-100"]["mediumWeight"] == 25, "default Label weight Light (stock, font-scale lib)")
check(rb["stock-bold"]["mediumWeight"] == 25, "stock library ignores UITK_FONT_BOLD")
check(rb["shim-bold"]["mediumWeight"] == 50 and rb["shim-bold"]["appWeight"] == 50, "UITK_FONT_BOLD=1: Label and application font Normal")
check(rb["shim-bold0"]["mediumWeight"] == 25, "UITK_FONT_BOLD=0: Light")
check(all(rb["shim-bold"][k] == base[k] for k in keys), "bold keeps the pixel sizes")
a_, b_ = img("shim-100"), img("shim-bold")
check(a_.size != b_.size or ImageChops.difference(a_, b_).getbbox() is not None, "bold grab differs from regular")
a_, b_ = img("stock"), img("shim-bold0")
check(a_.size == b_.size and ImageChops.difference(a_, b_).getbbox() is None, "UITK_FONT_BOLD=0 grab pixel-identical to stock")

# Live font-scale file (uitk-font-scale.patch watchUserScale).
def live(n):
    return (json.load(open(os.path.join(out, n + ".before.json"))),
            json.load(open(os.path.join(out, n + ".after.json"))))
lb, la = live("live-stock")
lkeys = ["medium", "large", "fontUtilsLarge"]
check(lb == la, "stock libLomiriToolkit ignores the font-scale file")
stock_live = lb
def scaled(sz, f):
    return all(abs(sz[k] - stock_live[k] * f) <= 1.0 for k in lkeys)
def unchanged(sz):
    return sz["fixed"] == stock_live["fixed"] and sz["gu"] == stock_live["gu"] and sz["box"] == stock_live["box"]
for n, fb, fa in [("live-up", 1.0, 1.3), ("live-down", 1.3, 1.0), ("live-start", 1.5, 1.5),
                  ("live-rm", 1.5, 1.15), ("live-bad", 1.0, 1.0), ("live-noenv", 1.0, 1.0)]:
    b_, a_ = live(n)
    check(scaled(b_, fb) and scaled(a_, fa),
          "%s: x%.2f -> x%.2f (%s -> %s)" % (n, fb, fa, [b_[k] for k in lkeys], [a_[k] for k in lkeys]))
    check(unchanged(b_) and unchanged(a_), n + ": explicit pixel size and grid unit unchanged")
for n in ["live-up", "live-down", "live-rm"]:
    check(ImageChops.difference(img(n + "-before"), img(n + "-after")).getbbox() is not None
          if img(n + "-before").size == img(n + "-after").size else True, n + ": the grab changed")
a, b = img("live-down-after"), img("live-stock-after")
check(a.size == b.size and ImageChops.difference(a, b).getbbox() is None, "live-down back to 100: grab pixel-identical to stock")

print("ALL PASS" if not fails else "FAILED: %d" % len(fails))
sys.exit(1 if fails else 0)
