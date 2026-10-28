# Laptop (build platform) Mir 2.15 exactly as the phone patches it
# (configuration.nix: mir-input-method-v1.patch), with and without
# mir-input-method-v1-own-modifiers-map.patch, for test/default.nix.
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  base = pkgs.mir_2_15.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ../../../mir-input-method-v1.patch ];
    doCheck = false;
  });
in {
  unpatched = base;
  patched = base.overrideAttrs (old: {
    patches = old.patches ++ [ ../mir-input-method-v1-own-modifiers-map.patch ];
  });
}
