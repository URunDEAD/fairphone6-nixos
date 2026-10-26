# Compile-only check of the staged sensors patches as EXTERNAL modules against
# the exact 7.2.0-fp6-nixos4 headers already present in the builder store.
# It never builds a kernel: both inputs are referenced with builtins.storePath,
# so evaluation fails instead of scheduling a kernel build if they are absent.
# The produced .ko files are evidence only; never load them next to the
# built-in ESWIN driver / in-tree stk3310.
#
# Run ON THE GUEST (store paths must exist there):
#   nix-build compile-check.nix --max-jobs 1 --cores 2 -o result-sensors-compile-check
{ nixpkgs ? builtins.fetchTarball {
    url = "https://releases.nixos.org/nixos/unstable/nixos-26.11pre1031299.0bb7ec54c848/nixexprs.tar.xz";
    sha256 = "sha256-nFgG4gPQueCgP8LidXsdI2lYPrSwTcmDl6afcVn7F/U=";
  }
, kernelSrc ? "/nix/store/i9iynws42nf1a5dcrvwvibj8q91czcwd-linux-63289f4411520a2acb38bd533437446d33fbce74.tar.gz"
, kernelDev ? "/nix/store/3fpx8xpl0yyzsipvdcr5jwjni5mc5wsv-linux-fp6-nixos-7.2.0-dev"
, release ? "7.2.0-fp6-nixos4"
}:
let
  pkgs = import nixpkgs { system = "aarch64-linux"; };
  llvm = pkgs.llvmPackages;
  src = builtins.storePath kernelSrc;
  dev = builtins.storePath kernelDev;
  top = "linux-63289f4411520a2acb38bd533437446d33fbce74";
  make = dir: ''
    make -j1 -C ${dev}/lib/modules/${release}/build M="$PWD/${dir}" LLVM=1 W=1 \
      CC=${llvm.clang-unwrapped}/bin/clang LD=${llvm.lld}/bin/ld.lld \
      AR=${llvm.llvm}/bin/llvm-ar NM=${llvm.llvm}/bin/llvm-nm \
      STRIP=${llvm.llvm}/bin/llvm-strip OBJCOPY=${llvm.llvm}/bin/llvm-objcopy \
      OBJDUMP=${llvm.llvm}/bin/llvm-objdump READELF=${llvm.llvm}/bin/llvm-readelf \
      HOSTCC=${llvm.clang}/bin/clang HOSTCXX=${llvm.clang}/bin/clang++ \
      SHELL=${llvm.stdenv.shell} modules 2>&1 | tee -a build-${builtins.baseNameOf dir}.log
    test "''${PIPESTATUS[0]}" = 0
  '';
in
llvm.stdenv.mkDerivation {
  pname = "fp6-sensors-feature-push-compile-check";
  version = "63289f441152-nixos4";
  inherit src;
  patches = [
    ./0001-iio-light-stk3310-noirq-polling-event-free.patch
    ./0002-iio-light-stk3310-retry-transient-bus-errors.patch
    ./0003-input-eswin-eph861x-gesture-wake-pm-lp-mode.patch
  ];
  nativeBuildInputs = [ pkgs.flex pkgs.bison pkgs.perl pkgs.bc pkgs.openssl
                        llvm.lld llvm.llvm pkgs.kmod ];
  hardeningDisable = [ "pic" ];
  unpackPhase = ''
    runHook preUnpack
    mkdir work
    tar -xf "$src" -C work --strip-components=1 \
      ${top}/drivers/iio/light/stk3310.c \
      ${top}/drivers/input/touchscreen/eswin_touch
    cd work
    runHook postUnpack
  '';
  # patches apply with -p1 relative to work/
  postPatch = ''
    mkdir -p m-stk m-eswin
    cp drivers/iio/light/stk3310.c m-stk/stk3310_fp6check.c
    echo 'obj-m := stk3310_fp6check.o' > m-stk/Makefile
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
    ${make "m-stk"}
    ${make "m-eswin"}
    runHook postBuild
  '';
  doCheck = true;
  checkPhase = ''
    for ko in m-stk/stk3310_fp6check.ko m-eswin/eswin_ts_fp6check.ko; do
      test "$(modinfo -F vermagic $ko | cut -d' ' -f1)" = ${release}
      llvm-readelf -h $ko | grep -q 'Machine:.*AArch64'
    done
    llvm-nm m-eswin/eswin_ts_fp6check.ko > eswin-symbols.txt
    grep -q eph_gesture_lp_enter eswin-symbols.txt
    grep -q eph_gesture_lp_exit eswin-symbols.txt
    grep -q dev_attr_lp_mode eswin-symbols.txt
    llvm-nm m-stk/stk3310_fp6check.ko > stk-symbols.txt
    grep -q stk3310_probe stk-symbols.txt
  '';
  installPhase = ''
    mkdir -p $out/share/fp6-sensors-compile-check
    cp m-stk/stk3310_fp6check.ko m-eswin/eswin_ts_fp6check.ko build-*.log \
      eswin-symbols.txt stk-symbols.txt $out/share/fp6-sensors-compile-check/
    sha256sum drivers/iio/light/stk3310.c drivers/input/touchscreen/eswin_touch/* \
      > $out/share/fp6-sensors-compile-check/patched-source-sha256.txt
  '';
  dontStrip = true;
  dontFixup = true;
}
