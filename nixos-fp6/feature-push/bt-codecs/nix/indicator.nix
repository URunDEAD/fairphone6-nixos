# The FP6 sound indicator, which carries the codec-restore patch
# (../../audio-routing/indicator-sound-package.nix).
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs {} }:
pkgs.callPackage ../../audio-routing/indicator-sound-package.nix {}
