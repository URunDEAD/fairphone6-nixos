import difflib, os, sys
old, new, header = sys.argv[1], sys.argv[2], sys.argv[3]
out = [header.rstrip("\n") + "\n"] if header else []
for dp, dn, fn in sorted(os.walk(new)):
    dn.sort()
    for f in sorted(x for x in fn if x.endswith((".qml", ".js"))):
        n = os.path.join(dp, f); rel = os.path.relpath(n, new); o = os.path.join(old, rel)
        a = open(o).read().splitlines(True) if os.path.exists(o) else []
        b = open(n).read().splitlines(True)
        if a == b: continue
        out += list(difflib.unified_diff(a, b, "a/" + rel, "b/" + rel, n=3))
sys.stdout.write("".join(out))
