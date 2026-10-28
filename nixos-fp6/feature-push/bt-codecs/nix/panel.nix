# The FP6 Bluetooth panel, which carries the audio-quality and pairing patches
# (../../bluetooth-settings/package.nix).
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs {} }:
(pkgs.callPackage ../../bluetooth-settings/package.nix {}).lssBluetooth
