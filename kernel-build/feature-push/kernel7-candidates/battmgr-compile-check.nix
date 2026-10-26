# Compile-only check of the kernel7 candidate qcom_battmgr patch (0002-...) as
# an EXTERNAL module against the exact kernel6 headers (7.2.0-fp6-nixos6), in
# the same way as compile-check.nix (no kernel build; inputs via
# builtins.storePath). The .ko is evidence only: the driver is built in
# (CONFIG_BATTERY_QCOM_BATTMGR=y); never load it.
#
# Run (from the repo root):
#   .macmini-builder/nix-remote build --eval-store daemon \
#     --file kernel-build/feature-push/kernel7-candidates/battmgr-compile-check.nix --no-link --print-out-paths
{ nixpkgs ? builtins.fetchTarball {
    url = "https://releases.nixos.org/nixos/unstable/nixos-26.11pre1031299.0bb7ec54c848/nixexprs.tar.xz";
    sha256 = "sha256-nFgG4gPQueCgP8LidXsdI2lYPrSwTcmDl6afcVn7F/U=";
  }
, kernelSrc ? "/nix/store/i9iynws42nf1a5dcrvwvibj8q91czcwd-linux-63289f4411520a2acb38bd533437446d33fbce74.tar.gz"
, kernelDev ? "/nix/store/3disx214wj8xd00imh7b83a58fa15jf0-linux-fp6-nixos-7.2.0-dev"
, release ? "7.2.0-fp6-nixos6"
}:
let
  pkgs = import nixpkgs { system = "aarch64-linux"; };
  llvm = pkgs.llvmPackages;
  src = builtins.storePath kernelSrc;
  dev = builtins.storePath kernelDev;
  top = "linux-63289f4411520a2acb38bd533437446d33fbce74";
in
llvm.stdenv.mkDerivation {
  pname = "fp6-kernel7-candidate-battmgr-compile-check";
  version = "63289f441152-nixos6";
  inherit src;
  # kernel6 order: the uAh patch is already in kernel6; 0002 goes on top.
  patches = [
    ../misc/0001-power-supply-qcom_battmgr-report-charge-in-uAh-on-SM8350-family.patch
    ./0002-power-supply-qcom_battmgr-charge-control-errors-and-disable.patch
  ];
  nativeBuildInputs = [ pkgs.flex pkgs.bison pkgs.perl pkgs.bc pkgs.openssl
                        llvm.lld llvm.llvm pkgs.kmod ];
  hardeningDisable = [ "pic" ];
  unpackPhase = ''
    runHook preUnpack
    mkdir work
    tar -xf "$src" -C work --strip-components=1 ${top}/drivers/power/supply/qcom_battmgr.c
    cd work
    runHook postUnpack
  '';
  postPatch = ''
    mkdir -p m-battmgr
    cp drivers/power/supply/qcom_battmgr.c m-battmgr/qcom_battmgr_fp6check.c
    echo 'obj-m := qcom_battmgr_fp6check.o' > m-battmgr/Makefile
  '';
  buildPhase = ''
    runHook preBuild
    set -o pipefail
    make -j2 -C ${dev}/lib/modules/${release}/build M="$PWD/m-battmgr" LLVM=1 W=1 \
      CC=${llvm.clang-unwrapped}/bin/clang LD=${llvm.lld}/bin/ld.lld \
      AR=${llvm.llvm}/bin/llvm-ar NM=${llvm.llvm}/bin/llvm-nm \
      STRIP=${llvm.llvm}/bin/llvm-strip OBJCOPY=${llvm.llvm}/bin/llvm-objcopy \
      OBJDUMP=${llvm.llvm}/bin/llvm-objdump READELF=${llvm.llvm}/bin/llvm-readelf \
      HOSTCC=${llvm.clang}/bin/clang HOSTCXX=${llvm.clang}/bin/clang++ \
      SHELL=${llvm.stdenv.shell} modules 2>&1 | tee build-battmgr.log
    runHook postBuild
  '';
  doCheck = true;
  checkPhase = ''
    ko=m-battmgr/qcom_battmgr_fp6check.ko
    test "$(modinfo -F vermagic $ko | cut -d' ' -f1)" = ${release}
    # No warnings from the changed charge-control code (lines ~660-740).
    ! grep -nE 'qcom_battmgr_fp6check.c:(6[6-9][0-9]|7[0-4][0-9]):.*warning' build-battmgr.log
  '';
  installPhase = ''
    mkdir -p $out/share/fp6-kernel7-candidate-battmgr
    cp m-battmgr/qcom_battmgr_fp6check.ko build-battmgr.log $out/share/fp6-kernel7-candidate-battmgr/
    sha256sum drivers/power/supply/qcom_battmgr.c > $out/share/fp6-kernel7-candidate-battmgr/patched-source-sha256.txt
  '';
  dontStrip = true;
  dontFixup = true;
}
