# Pinned source rebuild, separate from the proven Clang18 binary repack.
# Native Clang21 is cached from the FP6 kernel; Google NDK supplies target data.
{ lib, stdenvNoCC, fetchurl, python3, meson, ninja, pkg-config, bison, flex
, binutils, glslang, llvmPackages_21, callPackage, runCommand
, configureOnly ? false
}:
let
  ndk = callPackage ./ndk.nix {};
  mesa = fetchurl {
    url = "https://archive.mesa3d.org/mesa-26.1.4.tar.xz";
    sha256 = "072705caa9adf4740f1489194b13e278ad959166863b5271fe423a86353c9ab6";
  };
  drm = fetchurl {
    url = "https://dri.freedesktop.org/libdrm/libdrm-2.4.123.tar.xz";
    sha256 = "a2b98567a149a74b0f50e91e825f9c0315d86e7be9b74394dae8b298caadb79e";
  };
  python = python3.withPackages (p: [ p.mako p.pyyaml p.packaging ]);
  clang = llvmPackages_21.clang-unwrapped;
  llvm = llvmPackages_21.llvm;
  lld = llvmPackages_21.lld;
  # Intrinsic headers must match the compiler (Clang18 arm_neon.h is not
  # compatible with Clang21 builtins). Keep Android target compiler-rt from NDK.
  resource = runCommand "fp6-android-clang21-resource" {} ''
    mkdir -p $out
    ln -s "$(${clang}/bin/clang -print-resource-dir)/include" $out/include
    ln -s ${ndk}/android-ndk-r27d/toolchains/llvm/prebuilt/linux-x86_64/lib/clang/18/lib $out/lib
  '';
in stdenvNoCC.mkDerivation {
  pname = "fp6-waydroid-mesa-a810-source" + lib.optionalString configureOnly "-configure";
  version = "26.1.4";
  nativeBuildInputs = [ python meson ninja pkg-config bison flex binutils glslang ];
  dontUnpack = true;
  dontConfigure = true; # The shared Python recipe owns Meson cross configuration.
  dontFixup = true;
  buildPhase = ''
    runHook preBuild
    mkdir work
    cd work
    cp ${./build-support/build-android.py} build-android.py
    cp ${mesa} mesa-26.1.4.tar.xz
    cp ${drm} libdrm-2.4.123.tar.xz
    tar xf mesa-26.1.4.tar.xz
    # Mesa directly executes some Python generators; /usr/bin/env is absent
    # inside a Nix sandbox. Only build-script interpreter paths are rewritten.
    patchShebangs mesa-26.1.4
    export HOME="$TMPDIR/home"
    mkdir -p "$HOME"
    python3 build-android.py --ndk ${ndk}/android-ndk-r27d \
      --resource-dir ${resource} \
      --cc ${clang}/bin/clang --cpp ${clang}/bin/clang++ \
      --native-cc ${llvmPackages_21.clang}/bin/clang \
      --native-cpp ${llvmPackages_21.clang}/bin/clang++ \
      --lld ${lld}/bin/ld.lld --ar ${llvm}/bin/llvm-ar \
      --strip ${llvm}/bin/llvm-strip --consumer-imports ${./consumer-imports.json} \
      --jobs 2 ${lib.optionalString configureOnly "--configure-only"}
    runHook postBuild
  '';
  installPhase = if configureOnly then ''
    mkdir -p $out
    cp android-build/{android-aarch64.ini,native.ini,toolchain.json,probe-elf.txt} $out/
    cp android-build/mesa/meson-logs/meson-log.txt $out/
  '' else ''
    mkdir -p $out/share/fp6-waydroid-a810
    cp -r android-build/overlay-vendor/lib64 $out/
    python3 ${./validate-overlay.py} $out/lib64 \
      --consumer-imports ${./consumer-imports.json} \
      --output $out/share/fp6-waydroid-a810/artifacts.json
    cp android-build/toolchain.json $out/share/fp6-waydroid-a810/
    cp ${./provenance.json} $out/share/fp6-waydroid-a810/binary-reference-provenance.json
    python3 - "$out/share/fp6-waydroid-a810/provenance.json" <<'PY'
    import json,sys
    json.dump({'package_kind':'pinned Nix source build', 'mesa_version':'26.1.4',
      'compiler':'Nix native Clang21; NDK r27d target data', 'api':33,
      'jobs':2,'source_changes':'build-script interpreter paths only; driver sources unchanged','phone_hardware_acceptance':False,
      'warning':'Source rebuild does not inherit runtime proof of the Clang18 binary artifact.'},
      open(sys.argv[1],'w'),indent=2)
    PY
    cp mesa-26.1.4/docs/license.rst $out/share/fp6-waydroid-a810/
  '';
  passthru = {
    inherit ndk mesa drm configureOnly;
    isBinaryRepack = false;
    hardwareAcceptance = false;
  };
  meta = {
    description = "Pinned source Android API33 AArch64 Mesa Freedreno A810 vendor overlay";
    license = with lib.licenses; [ mit bsd2 asl20 ];
    platforms = [ "aarch64-linux" ];
  };
}
