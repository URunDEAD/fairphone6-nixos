# Laptop/builder build entry points for the audio routing feature.
#   nix-build build.nix -A indicator   (patched ayatana-indicator-sound)
#   nix-build build.nix -A telephony   (lomiri-telephony-service with all patches)
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs, system ? builtins.currentSystem }:
let
  pkgs = import nixpkgs { inherit system; };
in {
  indicator = pkgs.callPackage ./indicator-sound-package.nix { };
}
