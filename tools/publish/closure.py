"""Static import closure of FP6 Nix entry points.

Usage: FP6_CLOSURE_ROOT=<workspace> closure.py <entry.nix>...
Prints "F <file>" for every file reached through ./relative path literals,
"D <dir>" for referenced directories and "M <file> <literal>" for literals
that do not exist. Paths built with `workspace + "/x"` are not followed;
list such files in allowlist.txt explicitly.
"""
import os,re,sys
root=os.environ.get("FP6_CLOSURE_ROOT", os.getcwd())
start=[os.path.join(root,p) for p in sys.argv[1:]]
pat=re.compile(r'(?<![A-Za-z0-9_\-}])(\.\.?(?:/[A-Za-z0-9._+\-]+)+)')
seen=set(); dirs=set(); missing=set()
stack=list(start)
while stack:
    f=os.path.normpath(stack.pop())
    if f in seen: continue
    seen.add(f)
    if not f.endswith('.nix'): continue
    try: txt=open(f,errors='replace').read()
    except Exception as e: continue
    # strip comments
    txt=re.sub(r'(?m)(^|\s)#.*$','',txt)
    for m in pat.finditer(txt):
        p=os.path.normpath(os.path.join(os.path.dirname(f),m.group(1)))
        if os.path.isdir(p):
            if os.path.exists(os.path.join(p,'default.nix')): stack.append(os.path.join(p,'default.nix'))
            dirs.add(p)
        elif os.path.exists(p): stack.append(p)
        else: missing.add((f,m.group(1)))
for s in sorted(seen): print("F",os.path.relpath(s,root))
for d in sorted(dirs): print("D",os.path.relpath(d,root))
for f,m in sorted(missing): print("M",os.path.relpath(f,root),m)
