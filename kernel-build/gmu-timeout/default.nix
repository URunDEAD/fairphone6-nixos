# Separate diagnostic backport; the original645 recipe and source remain unchanged.
{ nixpkgs ? builtins.fetchTarball {
    url = "https://releases.nixos.org/nixos/unstable/nixos-26.11pre1031299.0bb7ec54c848/nixexprs.tar.xz";
    sha256 = "sha256-nFgG4gPQueCgP8LidXsdI2lYPrSwTcmDl6afcVn7F/U=";
  }, system ? "aarch64-linux"
}:
let
  pkgs = import nixpkgs { inherit system; };
  source = ./linux-63289f4411520a2acb38bd533437446d33fbce74.tar.gz;
  kernel = pkgs.linuxManualConfig {
    pname = "linux-fp6-nixos";
    version = "7.2.0";
    modDirVersion = "7.2.0-fp6-nixos2";
    src = assert builtins.hashFile "sha256" source == "1a1aba170899273cbb97bce2dc0713957340a7be2e2d187faab6f821d60687ba"; source;
    configfile = ./config.final;
    kernelPatches = [ { name = "gmu-fw-init-timeout-100ms"; patch = ./gmu-fw-init-timeout-100ms.patch; } ];
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
