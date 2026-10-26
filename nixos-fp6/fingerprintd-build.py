"""Build pinned fingerprintd with cached Clang21 and standard global headers.

Only replace upstream Crafter's unavailable import-std module; preserve all
named partitions, hardware/protocol code and eight pure upstream test suites.
No daemon, TEE, sensor, firmware or real biometric store is opened by this build.
"""
import os
from pathlib import Path
import shlex
import subprocess

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
unordered_map unordered_set utility variant vector expected bitset cerrno regex shared_mutex future semaphore""".split()
(root / "fingerprintd-std-headers.h").write_text("".join(f"#include <{h}>\n" for h in headers))

for path in [*root.glob("interfaces/*.cppm"), *root.glob("implementations/*.cpp"), *root.glob("tests/*/main.cpp")]:
    source = path.read_text()
    if "import std;" not in source:
        continue
    source = source.replace("import std;", "")
    include = '#include "fingerprintd-std-headers.h"\n'
    if "export module " in source:
        source = source.replace("module;", "module;\n" + include, 1) if "module;" in source else "module;\n" + include + source
    else:
        source = include + source
    path.write_text(source)

cxx = shlex.split(os.environ.get("CXX", "clang++"))
cc = shlex.split(os.environ.get("CC", "clang"))
qcomtee = Path(os.environ["QCOMTEE_SOURCE"]) / "libqcomtee"
gio_cflags = [x for x in shlex.split(subprocess.check_output(["pkg-config", "--cflags", "gio-2.0"], text=True)) if x.startswith(("-I", "-D"))]
gio_libs = shlex.split(subprocess.check_output(["pkg-config", "--libs", "gio-2.0"], text=True))
flags = ["-std=c++2c", "-O2", "-pthread", "-Wno-unused-command-line-argument", f"-I{root}", f"-I{qcomtee / 'include'}", f"-fprebuilt-module-path={pcm}", *gio_cflags]

def run(args):
    print(shlex.join(map(str, args)), flush=True)
    subprocess.run(list(map(str, args)), check=True)

qcomtee_objects = []
for name in ("qcomtee_object", "objects/mem_obj"):
    obj = objects / (name.replace("/", "-") + ".o")
    run([*cc, "-O2", "-fPIC", f"-I{qcomtee / 'include'}", f"-I{qcomtee / 'src'}", "-c", qcomtee / "src" / (name + ".c"), "-o", obj])
    qcomtee_objects.append(obj)

core = []
module_flags = []
for name in ("Sfs", "Rpmb", "Ta", "Engine", "Store", "Tee", "Sensor", "Actions", None):
    stem = "Fingerprintd" + ("-" + name if name else "")
    module_name = "Fingerprintd" + (":" + name if name else "")
    compiled = pcm / (stem + ".pcm")
    obj = objects / (stem + ".o")
    run([*cxx, *flags, *module_flags, "--precompile", root / "interfaces" / (stem + ".cppm"), "-o", compiled])
    run([*cxx, *flags, *module_flags, "-c", compiled, "-o", obj])
    module_flags.append(f"-fmodule-file={module_name}={compiled}")
    core.append(obj)

for name, extra in (("main", qcomtee_objects), ("agent", [])):
    output = programs / ("fingerprintd" if name == "main" else "fingerprintd-agent")
    run([*cxx, *flags, *module_flags, root / "implementations" / (name + ".cpp"), *core, *extra, *gio_libs, "-o", output])

for suite in ("Sfs", "Rpmb", "Ta", "Engine", "Store", "Tee", "Sensor", "Actions"):
    binary = programs / ("test-" + suite)
    run([*cxx, *flags, *module_flags, root / "tests" / suite / "main.cpp", *core, "-o", binary])
    run([binary])
run([programs / "fingerprintd", "--version"])
print("Fingerprintd: all eight pure suites and version check passed", flush=True)
