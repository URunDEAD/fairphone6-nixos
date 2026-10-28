{ pkgs ? import (import ../../upstream/mobile-nixos/npins).nixpkgs {} }:
pkgs.callPackage ./package.nix {}
