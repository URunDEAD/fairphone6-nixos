# kernel6 = exact kernel5 recipe + UFS system-suspend fix for milos (spm_lvl 5 default,
# ../suspend/0001-...) + CONFIG_PM_DEBUG (/sys/power/pm_test). Kernel5 stays untouched.
{ nixpkgs ? builtins.fetchTarball {
    url = "https://releases.nixos.org/nixos/unstable/nixos-26.11pre1031299.0bb7ec54c848/nixexprs.tar.xz";
    sha256 = "sha256-nFgG4gPQueCgP8LidXsdI2lYPrSwTcmDl6afcVn7F/U=";
  }, system ? "aarch64-linux"
}:
let
  pkgs = import nixpkgs { inherit system; };
  source = ../../gmu-timeout/linux-63289f4411520a2acb38bd533437446d33fbce74.tar.gz;
  kernel = pkgs.linuxManualConfig {
    pname = "linux-fp6-nixos";
    version = "7.2.0";
    modDirVersion = "7.2.0-fp6-nixos6";
    src = assert builtins.hashFile "sha256" source == "1a1aba170899273cbb97bce2dc0713957340a7be2e2d187faab6f821d60687ba"; source;
    configfile = ./config.final;
    kernelPatches = [
      { name = "gmu-fw-init-timeout-100ms"; patch = ../../gmu-timeout/gmu-fw-init-timeout-100ms.patch; }
      { name = "qcomtee-large-shm-cma"; patch = ../../qcomtee-cma/qcomtee-large-shm-cma.patch; }
      { name = "milos-dphy-no-cphy-rate-table"; patch = ../../selfie-dphy/milos-dphy-no-cphy-rate-table.patch; }
      { name = "qcom-battmgr-charge-uah"; patch = ../misc/0001-power-supply-qcom_battmgr-report-charge-in-uAh-on-SM8350-family.patch; }
      { name = "stk3310-noirq-polling"; patch = ../sensors/0001-iio-light-stk3310-noirq-polling-event-free.patch; }
      { name = "stk3310-retry-bus-errors"; patch = ../sensors/0002-iio-light-stk3310-retry-transient-bus-errors.patch; }
      { name = "eswin-eph861x-gesture-wake"; patch = ../sensors/0003-input-eswin-eph861x-gesture-wake-pm-lp-mode.patch; }
      { name = "ufs-qcom-milos-spm-lvl5"; patch = ../suspend/0001-scsi-ufs-qcom-milos-power-down-link-in-system-suspend.patch; }
    ];
    stdenv = pkgs.llvmPackages.stdenv;
    # Nixpkgs sets absolute GNU tool paths even when LLVM=1; override the
    # target tools explicitly so ThinLTO uses the matching LLVM toolchain.
    extraMakeFlags = [
      "LLVM=1"
      "LD=${pkgs.llvmPackages.lld}/bin/ld.lld --thinlto-jobs=2"
      "AR=${pkgs.llvmPackages.llvm}/bin/llvm-ar"
      "NM=${pkgs.llvmPackages.llvm}/bin/llvm-nm"
      "STRIP=${pkgs.llvmPackages.llvm}/bin/llvm-strip"
      "OBJCOPY=${pkgs.llvmPackages.llvm}/bin/llvm-objcopy"
      "OBJDUMP=${pkgs.llvmPackages.llvm}/bin/llvm-objdump"
      "READELF=${pkgs.llvmPackages.llvm}/bin/llvm-readelf"
      "HOSTCC=${pkgs.llvmPackages.clang}/bin/clang"
      "HOSTCXX=${pkgs.llvmPackages.clang}/bin/clang++"
      # Host tools need the wrapped linker to retain dependency RPATHs.
      "HOSTLDFLAGS=--ld-path=${pkgs.buildPackages.stdenv.cc.bintools}/bin/ld"
    ];
    target = "Image.gz";
    buildDTBs = true;
    features.efiBootStub = true;
  };
in kernel.overrideAttrs (old: {
  nativeBuildInputs = (old.nativeBuildInputs or []) ++ [
    pkgs.llvmPackages.lld
    pkgs.llvmPackages.llvm
  ];
  # EFI_ZBOOT changes arm64 zinstall's default to vmlinuz.efi. Install the
  # explicitly built gzip image used by the FP6 Android boot image instead.
  installFlags = (old.installFlags or []) ++ [
    "KBUILD_IMAGE=arch/arm64/boot/Image.gz"
  ];
  passthru = old.passthru // { file = "Image.gz"; isQcdt = false; isExynosDT = false; };
})
