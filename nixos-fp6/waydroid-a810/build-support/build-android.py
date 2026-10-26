#!/usr/bin/env python3
"""Experimental standalone A810 Android build. Never accesses a phone.

Needs an extracted Google NDK and native-host clang/clang++/lld/llvm tools,
Meson >=1.4, Ninja, pkg-config, bison, flex, Python Mako/PyYAML/packaging.
Run only after the independent host-Mesa job has enough resources.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
SOURCE = ROOT / "mesa-26.1.4"


def run(args, **kwargs):
    print("+", " ".join(map(str, args)), flush=True)
    return subprocess.run(list(map(str, args)), check=True, **kwargs)


def tool(name):
    found = shutil.which(name)
    if not found:
        raise SystemExit(f"Missing native host tool: {name}")
    # LLVM chooses C++ and linker personalities from argv[0]; keep symlink names.
    return os.path.abspath(found)


def symbols(path, undefined=False):
    result = subprocess.check_output(["readelf", "--dyn-syms", "--wide", path], text=True)
    return {line.split()[-1].split("@")[0] for line in result.splitlines()
            if len(line.split()) >= 8 and ((" UND " in line) == undefined)}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--ndk", type=Path, required=True)
    p.add_argument("--cc", default="clang-18")
    p.add_argument("--cpp", default="clang++-18")
    p.add_argument("--lld", default="ld.lld-18")
    p.add_argument("--ar", default="llvm-ar-18")
    p.add_argument("--strip", default="llvm-strip-18")
    p.add_argument("--native-cc", help="Native code generator compiler (defaults to --cc)")
    p.add_argument("--native-cpp", help="Native code generator compiler (defaults to --cpp)")
    p.add_argument("--consumer-imports", type=Path,
                   help="Recorded public baseline GBM imports instead of vendor binaries")
    p.add_argument("--resource-dir", type=Path,
                   help="Compiler-matched intrinsic headers plus Android compiler-rt data")
    p.add_argument("--jobs", type=int, default=2)
    p.add_argument("--smoke-only", action="store_true")
    p.add_argument("--configure-only", action="store_true")
    args = p.parse_args()
    if args.jobs < 1:
        p.error("--jobs must be positive")
    ndk = args.ndk.resolve()
    prebuilts = list((ndk / "toolchains/llvm/prebuilt").glob("*"))
    if len(prebuilts) != 1:
        raise SystemExit("Expected exactly one NDK prebuilt directory")
    ndk_tools = prebuilts[0]
    sysroot = ndk_tools / "sysroot"
    resources = list((ndk_tools / "lib/clang").glob("*"))
    if len(resources) != 1:
        raise SystemExit("Expected exactly one NDK Clang resource directory")
    resource = resources[0]
    if args.resource_dir:
        resource = args.resource_dir.resolve()
        if not (resource / 'include/arm_neon.h').is_file() or not (resource / 'lib').is_dir():
            raise SystemExit('Resource override must contain intrinsic headers and Android runtime data')
    if not (sysroot / "usr/lib/aarch64-linux-android/33/crtbegin_so.o").exists():
        raise SystemExit("NDK lacks the API33 AArch64 CRT")
    cc, cpp, linker = tool(args.cc), tool(args.cpp), tool(args.lld)
    work = ROOT / "android-build"
    work.mkdir(exist_ok=True)
    # NDK provides zlib headers and the API-level libz link stub, but no .pc.
    # Freedreno's RD dump code requires zlib even with shader cache disabled.
    zlib_version = re.search(r'^#define ZLIB_VERSION "([^"]+)"',
                             (sysroot / "usr/include/zlib.h").read_text(), re.M)[1]
    pkgconfig = work / "pkgconfig"
    pkgconfig.mkdir(exist_ok=True)
    (pkgconfig / "zlib.pc").write_text(
        f"Name: zlib\nDescription: Android NDK zlib\nVersion: {zlib_version}\nLibs: -lz\n")
    common = ["--target=aarch64-linux-android33", f"--sysroot={sysroot}",
              f"-resource-dir={resource}", "-fPIC"]
    c = [cc, *common]
    # -nostdinc++ plus explicit NDK headers needs no -stdlib during compilation.
    # Otherwise Meson's -Werror=unused-command-line-argument feature probes
    # reject every C++ option, including upstream Turnip's narrowing policy.
    cxx = [cpp, *common, "-nostdinc++", "-isystem",
           str(sysroot / "usr/include/c++/v1"), "-fno-exceptions", "-fno-rtti"]
    link = ["-rtlib=compiler-rt", f"-fuse-ld={linker}"]
    # Hide only the static C++ runtime. ALL would also hide Mesa's archived
    # public dispatch and Vulkan HAL symbols, producing unusable shared libs.
    cxx_link = [*link, "-stdlib=libc++", "-static-libstdc++",
                "-Wl,--exclude-libs,libc++_static.a:libc++abi.a:libunwind.a"]
    (work / "probe.cpp").write_text(
        '#include <vector>\n#include <android/api-level.h>\n'
        'extern "C" int probe() { std::vector<int> a{1,2}; return a.size(); }\n')
    run([*cxx, *cxx_link, "-shared", work / "probe.cpp", "-o", work / "probe.so"])
    elf = subprocess.check_output(["readelf", "-h", "-d", work / "probe.so"], text=True)
    (work / "probe-elf.txt").write_text(elf)
    if "AArch64" not in elf or "libc.so.6" in elf or "libc++_shared.so" in elf:
        raise SystemExit("Toolchain did not produce the intended Android AArch64 ELF")
    cross = work / "android-aarch64.ini"
    cross.write_text(
        "[binaries]\n"
        f"c = {c!r}\ncpp = {cxx!r}\n"
        f"ar = {tool(args.ar)!r}\nstrip = {tool(args.strip)!r}\n"
        "pkg-config = 'pkg-config'\n"
        "[host_machine]\nsystem = 'android'\ncpu_family = 'aarch64'\n"
        "cpu = 'armv8-a'\nendian = 'little'\n"
        "[properties]\nneeds_exe_wrapper = true\n"
        f"pkg_config_libdir = [{str(pkgconfig)!r}]\n"
        f"[built-in options]\nc_link_args = {link!r}\ncpp_link_args = {cxx_link!r}\n")
    native = work / "native.ini"
    native.write_text(f"[binaries]\nc = {tool(args.native_cc) if args.native_cc else cc!r}\n"
                      f"cpp = {tool(args.native_cpp) if args.native_cpp else cpp!r}\n")
    (work / "toolchain.json").write_text(json.dumps({
        "ndk": str(ndk), "c": c, "cpp": cxx, "c_link_args": link,
        "cpp_link_args": cxx_link,
        "native_clang": subprocess.check_output([cc, "--version"], text=True),
        "ndk_source_properties": (ndk / "source.properties").read_text(),
        "status": "toolchain link smoke passed; Android runtime untested",
    }, indent=2) + "\n")
    if args.smoke_only:
        return
    archive = ROOT / "mesa-26.1.4.tar.xz"
    expected = "072705caa9adf4740f1489194b13e278ad959166863b5271fe423a86353c9ab6"
    if hashlib.sha256(archive.read_bytes()).hexdigest() != expected:
        raise SystemExit("Mesa source archive hash mismatch")
    if (SOURCE / "VERSION").read_text().strip() != "26.1.4":
        raise SystemExit("Wrong extracted Mesa version")
    device_source = (SOURCE / "src/freedreno/common/freedreno_devices.py").read_text()
    if 'chip_id=0xffff44010000, name="Adreno (TM) 810"' not in device_source:
        raise SystemExit("A810 device definition is missing")
    packagecache = SOURCE / "subprojects/packagecache"
    packagecache.mkdir(exist_ok=True)
    drm = ROOT / "libdrm-2.4.123.tar.xz"
    if hashlib.sha256(drm.read_bytes()).hexdigest() != "a2b98567a149a74b0f50e91e825f9c0315d86e7be9b74394dae8b298caadb79e":
        raise SystemExit("libdrm source hash mismatch")
    shutil.copy2(drm, packagecache / drm.name)
    build = work / "mesa"
    flags = ["--buildtype=release", "--prefix=/vendor", "--libdir=lib64",
             "-Dc_link_args=" + json.dumps(link),
             "-Dcpp_link_args=" + json.dumps(cxx_link),
             "--cross-file", str(cross), "--wrap-mode=nodownload",
             "--native-file", str(native),
             "--force-fallback-for=libdrm", "-Dlibdrm:default_library=static",
             "-Dallow-fallback-for=libdrm",
             "-Dauto_features=disabled", "-Dplatforms=android", "-Dplatform-sdk-version=33",
             "-Dandroid-stub=true", "-Dandroid-strict=false", "-Dfreedreno-kmds=msm",
             "-Dgallium-drivers=freedreno", "-Dvulkan-drivers=freedreno",
             "-Degl=enabled", "-Dgles1=enabled", "-Dgles2=enabled", "-Dgbm=enabled",
             "-Dgbm-backends-path=/vendor/lib64", "-Dllvm=disabled",
             "-Dzlib=enabled", "-Dzstd=disabled", "-Dshader-cache=disabled",
             "-Dexpat=disabled", "-Dbuild-tests=false"]
    if (build / "meson-private/coredata.dat").exists():
        compilers = json.loads(subprocess.check_output(
            ["meson", "introspect", "--compilers", build], text=True))
        # Meson preserves cached compiler argv on reconfigure, even if the
        # cross file changes. Reset only for a genuine compiler-command change.
        if compilers["host"]["cpp"]["exelist"] != cxx:
            print("Compiler argv changed; resetting Meson compiler cache.", flush=True)
            run(["meson", "setup", "--wipe", build, SOURCE, *flags])
        else:
            run(["meson", "setup", "--reconfigure", "--clearcache", build, SOURCE, *flags])
    else:
        run(["meson", "setup", build, SOURCE, *flags])
    if args.configure_only:
        print("Meson configuration passed; Mesa compilation was not started.")
        return
    run(["meson", "compile", "-C", build, "-j", args.jobs])
    dest = work / "install"
    env = dict(os.environ, DESTDIR=str(dest))
    run(["meson", "install", "-C", build, "--no-rebuild"], env=env)
    installed = dest / "vendor/lib64"
    stage = work / "overlay-vendor/lib64"
    mapping = {
        "libgallium_dri.so": "libgallium_dri.so",
        "libgbm_mesa.so": "libgbm_mesa.so",
        "gbm/dri_gbm.so": "dri_gbm.so",
        "libEGL.so": "egl/libEGL_mesa.so",
        "libGLESv1_CM.so": "egl/libGLESv1_CM_mesa.so",
        "libGLESv2.so": "egl/libGLESv2_mesa.so",
        "libvulkan_freedreno.so": "hw/vulkan.freedreno.so",
    }
    expected_sonames = {
        "libgallium_dri.so": "libgallium_dri.so",
        "libgbm_mesa.so": "libgbm_mesa.so", "dri_gbm.so": "dri_gbm.so",
        "egl/libEGL_mesa.so": "libEGL.so",
        "egl/libGLESv1_CM_mesa.so": "libGLESv1_CM.so",
        "egl/libGLESv2_mesa.so": "libGLESv2.so",
        "hw/vulkan.freedreno.so": "libvulkan_freedreno.so",
    }
    evidence = {}
    for src, target in mapping.items():
        source, output = installed / src, stage / target
        if not source.exists():
            raise SystemExit(f"Expected Mesa artifact missing: {source}")
        output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, output, follow_symlinks=True)
        details = subprocess.check_output(["readelf", "-h", "-d", output], text=True)
        if "AArch64" not in details or "libc.so.6" in details or "libc++_shared.so" in details:
            raise SystemExit(f"Unexpected runtime ABI in {output}")
        if "RUNPATH" in details or "RPATH" in details:
            raise SystemExit(f"Remove build-only RPATH before accepting {output}")
        if f'Library soname: [{expected_sonames[target]}]' not in details:
            raise SystemExit(f"Unexpected SONAME in {output}")
        evidence[target] = {"sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
                            "elf": details}
    # Existing gralloc consumers must still find every imported GBM symbol.
    exports = symbols(stage / "libgbm_mesa.so")
    if args.consumer_imports:
        consumers = json.loads(args.consumer_imports.read_text())
    else:
        consumers = {str(p): sorted(s for s in symbols(p, undefined=True)
                                   if s.startswith("gbm_")) for p in
                     [ROOT / "vendor-libs/libgbm_mesa_wrapper.so",
                      ROOT / "vendor-libs/hw/gralloc.gbm.so"]}
    for consumer, imports in consumers.items():
        needed = set(imports)
        if needed - exports:
            raise SystemExit(f"GBM ABI mismatch for {consumer}: {sorted(needed - exports)}")
    (work / "artifacts.json").write_text(json.dumps(evidence, indent=2) + "\n")
    if (ROOT / "android-runtime/provenance.json").exists():
        run([sys.executable, ROOT / "validate-android-abi.py"])
    print("Build/ELF/GBM checks passed. No phone/runtime tests have been performed.")
    print("Review overlay-vendor and Android runtime symbols before using on a phone.")


if __name__ == "__main__":
    main()
