# Standalone eval helper; the full kernel6 target imports module.nix itself.
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ../trial-lomiri-full-kernel6-fp6.nix { inherit nixpkgs; };
in base
