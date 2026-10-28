# Compile-only check of the kernel7 candidate panel patches
# (0009 modes 120/90/60/30 Hz, 0010 runtime panel_rate_hz) on the
# LAPTOP (x86_64 host, clang cross-compiling for arm64; no ARM builder).
# It never builds a kernel image or modules: it configures the pinned
# kernel6 source with kernel6's config.final and compiles the one object
# drivers/gpu/drm/panel/panel-novatek-nt37705.o with W=1. The object is
# evidence only (ThinLTO bitcode); never load or ship it.
#
# Run (from the repo root):
#   nix-build kernel-build/feature-push/kernel7-candidates/panel-refresh-compile-check.nix
{ nixpkgs ? builtins.fetchTarball {
    url = "https://releases.nixos.org/nixos/unstable/nixos-26.11pre1031299.0bb7ec54c848/nixexprs.tar.xz";
    sha256 = "sha256-nFgG4gPQueCgP8LidXsdI2lYPrSwTcmDl6afcVn7F/U=";
  }
}:
let
  pkgs = import nixpkgs { system = "x86_64-linux"; };
  llvm = pkgs.llvmPackages;
  source = ../../gmu-timeout/linux-63289f4411520a2acb38bd533437446d33fbce74.tar.gz;
  obj = "drivers/gpu/drm/panel/panel-novatek-nt37705.o";
  make = ''make ARCH=arm64 LLVM=1 \
      CC=${llvm.clang-unwrapped}/bin/clang LD=${llvm.lld}/bin/ld.lld \
      AR=${llvm.llvm}/bin/llvm-ar NM=${llvm.llvm}/bin/llvm-nm \
      OBJCOPY=${llvm.llvm}/bin/llvm-objcopy OBJDUMP=${llvm.llvm}/bin/llvm-objdump \
      READELF=${llvm.llvm}/bin/llvm-readelf STRIP=${llvm.llvm}/bin/llvm-strip \
      HOSTCC=${llvm.clang}/bin/clang HOSTCXX=${llvm.clang}/bin/clang++ \
      SHELL=${pkgs.bash}/bin/bash'';
in
pkgs.stdenvNoCC.mkDerivation {
  pname = "fp6-kernel7-candidate-nt37705-refresh-compile-check";
  version = "63289f441152-nixos6";
  src = assert builtins.hashFile "sha256" source == "1a1aba170899273cbb97bce2dc0713957340a7be2e2d187faab6f821d60687ba"; source;
  patches = [
    ./0009-drm-panel-novatek-nt37705-add-90-60-and-30-hz-modes.patch
    ./0010-drm-panel-novatek-nt37705-runtime-panel-rate-without-modeset.patch
  ];
  nativeBuildInputs = [ pkgs.flex pkgs.bison pkgs.perl pkgs.bc pkgs.openssl
                        pkgs.gnumake pkgs.python3 llvm.clang llvm.lld llvm.llvm ];
  hardeningDisable = [ "all" ];
  dontConfigure = true;
  postPatch = ''
    patchShebangs scripts
  '';
  buildPhase = ''
    runHook preBuild
    set -o pipefail
    cp ${../kernel6/config.final} .config
    ${make} olddefconfig
    # Same options as kernel6 for this driver.
    grep -qx 'CONFIG_DRM_PANEL_NOVATEK_NT37705=m' .config
    ${make} -j$NIX_BUILD_CORES W=1 ${obj} 2>&1 | tee build-panel.log
    runHook postBuild
  '';
  doCheck = true;
  checkPhase = ''
    test -s ${obj}
    # No warnings or errors from the patched driver.
    ! grep -n 'panel-novatek-nt37705.c:[0-9]*:[0-9]*: \(warning\|error\)' build-panel.log
  '';
  installPhase = ''
    mkdir -p $out/share/fp6-kernel7-candidate-nt37705-refresh
    cp ${obj} build-panel.log $out/share/fp6-kernel7-candidate-nt37705-refresh/
    sha256sum drivers/gpu/drm/panel/panel-novatek-nt37705.c \
      > $out/share/fp6-kernel7-candidate-nt37705-refresh/patched-source-sha256.txt
  '';
  dontStrip = true;
  dontFixup = true;
}
