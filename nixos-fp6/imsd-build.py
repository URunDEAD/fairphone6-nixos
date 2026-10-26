"""Build pinned imsd with cached Clang and system libstdc++.

The upstream Crafter SDK provides `import std;`. Nixpkgs does not ship its
prebuilt std module. Use standard headers in the global module fragment;
retain every named imsd module and its implementation without protocol edits.
"""
import os
from pathlib import Path
import shlex
import subprocess
import sys

root = Path.cwd()
pcm = root / "pcm"
objects = root / "objects"
programs = root / "programs"
for directory in (pcm, objects, programs):
    directory.mkdir(exist_ok=True)

headers = """algorithm any array atomic bit cassert cctype charconv chrono cmath
concepts condition_variable cstddef cstdint cstdio cstdlib cstring deque
exception filesystem format fstream functional iomanip ios iostream iterator
limits map memory mutex numbers numeric optional print queue random ranges
set span sstream stdexcept string string_view thread tuple type_traits
unordered_map unordered_set utility variant vector""".split()
(root / "imsd-std-headers.h").write_text("".join(f"#include <{h}>\n" for h in headers))

for path in [*root.glob("interfaces/*.cppm"), *root.glob("implementations/*.cpp"), *root.glob("tests/*/main.cpp")]:
    source = path.read_text()
    if "import std;" not in source:
        continue
    source = source.replace("import std;", "")
    include = '#include "imsd-std-headers.h"\n'
    if "export module " in source:
        if "module;" in source:
            source = source.replace("module;", "module;\n" + include, 1)
        else:
            source = "module;\n" + include + source
    else:
        source = include + source
    path.write_text(source)

cxx = shlex.split(os.environ.get("CXX", "clang++"))
gio_cflags = [x for x in shlex.split(subprocess.check_output(["pkg-config", "--cflags", "gio-2.0"], text=True)) if x.startswith(("-I", "-D"))]
gio_libs = shlex.split(subprocess.check_output(["pkg-config", "--libs", "gio-2.0"], text=True))
flags = ["-std=c++2c", "-O2", "-pthread", "-Wno-unused-command-line-argument", f"-I{root}", f"-fprebuilt-module-path={pcm}", *gio_cflags]


def run(args):
    print(shlex.join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), check=True)


core = []
for name in ("Util", "Aka", "Ipsec", "Qmi", "Sip", "Sdp", "Messages", "Engine"):
    module = f"Imsd-{name}"
    run([*cxx, *flags, "--precompile", root / f"interfaces/{module}.cppm", "-o", pcm / f"{module}.pcm"])
    obj = objects / f"{module}.o"
    run([*cxx, *flags, "-c", pcm / f"{module}.pcm", "-o", obj])
    core.append(obj)
for name in ("Imsd", "ImsdBus"):
    run([*cxx, *flags, "--precompile", root / f"interfaces/{name}.cppm", "-o", pcm / f"{name}.pcm"])
    run([*cxx, *flags, "-c", pcm / f"{name}.pcm", "-o", objects / f"{name}.o"])
core.append(objects / "Imsd.o")
bus = objects / "ImsdBus.o"
products = {
    "imsd": ("main", [*core, bus], gio_libs),
    "imsd-pcscf": ("pcscf", core, []),
    "imsd-media": ("media", [], ["-ldl", "-lpthread"]),
    "imsd-dialerd": ("dialerd", [], gio_libs),
    "imsd-ofonod": ("ofonod", [bus], gio_libs),
}
for executable, (source, libraries, link_flags) in products.items():
    run([*cxx, *flags, root / f"implementations/{source}.cpp", *libraries, *link_flags, "-pthread", "-o", programs / executable])
if "--test" in sys.argv:
    for suite in ("Util", "Aka", "Ipsec", "Qmi", "Messages", "Engine", "Sip", "Sdp"):
        binary = programs / f"test-{suite}"
        run([*cxx, *flags, root / f"tests/{suite}/main.cpp", *core, "-pthread", "-o", binary])
        run([binary])
    run([programs / "imsd-media", "--selftest"])
