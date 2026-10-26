# Compile-only check of the kernel7 candidate ESWIN patch (0001-...) as an
# EXTERNAL module against the exact kernel6 headers (7.2.0-fp6-nixos6) already
# in the builder store. It never builds a kernel: both inputs are referenced
# with builtins.storePath, so evaluation fails instead of scheduling a kernel
# build if they are absent. The .ko is evidence only; never load it next to
# the built-in ESWIN driver.
#
# Run (from the repo root):
#   .macmini-builder/nix-remote build --eval-store daemon \
#     --file kernel-build/feature-push/kernel7-candidates/compile-check.nix --no-link --print-out-paths
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
  pname = "fp6-kernel7-candidate-eswin-compile-check";
  version = "63289f441152-nixos6";
  inherit src;
  # Same order as kernel6's kernelPatches (0003 touches the same file).
  patches = [
    ../sensors/0003-input-eswin-eph861x-gesture-wake-pm-lp-mode.patch
    ./0001-input-eswin-eph861x-wait-for-reset-report-in-probe.patch
  ];
  nativeBuildInputs = [ pkgs.flex pkgs.bison pkgs.perl pkgs.bc pkgs.openssl
                        llvm.lld llvm.llvm pkgs.kmod ];
  hardeningDisable = [ "pic" ];
  unpackPhase = ''
    runHook preUnpack
    mkdir work
    tar -xf "$src" -C work --strip-components=1 ${top}/drivers/input/touchscreen/eswin_touch
    cd work
    runHook postUnpack
  '';
  postPatch = ''
    mkdir -p m-eswin
    cp drivers/input/touchscreen/eswin_touch/* m-eswin/
    cat > m-eswin/Makefile <<'MAKE'
    obj-m := eswin_ts_fp6check.o
    eswin_ts_fp6check-y := eswin_eph861x.o eswin_eph861x_bootloader.o \
      eswin_eph861x_comms.o eswin_eph861x_eswin.o eswin_eph861x_i2c.o \
      eswin_eph861x_spi.o eswin_eph861x_tlv_command.o eswin_eph861x_tlv_report.o
    MAKE
  '';
  buildPhase = ''
    runHook preBuild
    set -o pipefail
    make -j2 -C ${dev}/lib/modules/${release}/build M="$PWD/m-eswin" LLVM=1 W=1 \
      CC=${llvm.clang-unwrapped}/bin/clang LD=${llvm.lld}/bin/ld.lld \
      AR=${llvm.llvm}/bin/llvm-ar NM=${llvm.llvm}/bin/llvm-nm \
      STRIP=${llvm.llvm}/bin/llvm-strip OBJCOPY=${llvm.llvm}/bin/llvm-objcopy \
      OBJDUMP=${llvm.llvm}/bin/llvm-objdump READELF=${llvm.llvm}/bin/llvm-readelf \
      HOSTCC=${llvm.clang}/bin/clang HOSTCXX=${llvm.clang}/bin/clang++ \
      SHELL=${llvm.stdenv.shell} modules 2>&1 | tee build-eswin.log
    runHook postBuild
  '';
  doCheck = true;
  checkPhase = ''
    ko=m-eswin/eswin_ts_fp6check.ko
    test "$(modinfo -F vermagic $ko | cut -d' ' -f1)" = ${release}
    modinfo -F parm $ko | tee eswin-parm.txt
    grep -q '^ic_boot_timeout_ms:' eswin-parm.txt
    grep -q '^ic_boot_settle_ms:' eswin-parm.txt
    # No new warnings from the changed probe code.
    ! grep -n 'eswin_eph861x.c:22[0-9][0-9]:.*warning' build-eswin.log
  '';
  installPhase = ''
    mkdir -p $out/share/fp6-kernel7-candidate-eswin
    cp m-eswin/eswin_ts_fp6check.ko build-eswin.log eswin-parm.txt $out/share/fp6-kernel7-candidate-eswin/
    sha256sum drivers/input/touchscreen/eswin_touch/eswin_eph861x.c \
      > $out/share/fp6-kernel7-candidate-eswin/patched-source-sha256.txt
  '';
  dontStrip = true;
  dontFixup = true;
}
