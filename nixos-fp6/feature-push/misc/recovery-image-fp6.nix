# FP6 RAM-boot recovery image, pinned to a known-good generation.
# Changes ONLY the cmdline of a physically accepted boot image (same kernel,
# DTB, initrd, mobile-nixos.generation) -> multi-user.target + fp6.recovery=1.
# Architecture-independent (pure Python byte edit); build anywhere:
#   nix-build nixos-fp6/feature-push/misc/recovery-image-fp6.nix
# Use only with tools/fastboot-ram.py (fastboot boot). NEVER fastboot flash.
{ pkgs ? import <nixpkgs> { }
, source ? ../../../.macmini-builder/native-final/phosh-selfie-rdi16/svvbg0n7vgk7lmn5glrrxzr4phnm1v2w-fp6-nixos-selfie-dphy-kernel4-trial.img
, sourceSha256 ? "46bc4f54181ab84da7781a9f5e7688a5b5d1df7f468be48e91197e9ae940af00"
}:
let
  src = builtins.path { path = source; name = "fp6-accepted-boot.img"; sha256 = builtins.convertHash { hash = sourceSha256; hashAlgo = "sha256"; toHashFormat = "nix32"; }; recursive = false; };
in
pkgs.runCommand "fp6-recovery-boot-img" { nativeBuildInputs = [ pkgs.python3 ]; } ''
  mkdir -p $out
  python3 ${../../../tools/feature-push/misc-make-recovery-image.py} \
    --source ${src} --source-sha256 ${sourceSha256} $out/recovery.img > $out/build.log
''
