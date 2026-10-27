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

print("ALL PASS" if not fails else "FAILED: %d" % len(fails))
sys.exit(1 if fails else 0)
