# Runs only unpack/patch/postPatch of the target's (aarch64) lomiri on the
# laptop and keeps the patched tree: checks that every patch layer applies and
# gives test/preview.sh the patched QML.
#   nix-build test/patchcheck.nix -o /tmp/fp6-lomiri-patched
{ target ? ../../trial-lomiri-full-kernel6-fp6.nix
, attr ? "lomiri.lomiri" }:
let
  t = import target { };
  lp = import (import ../../../upstream/mobile-nixos/npins).nixpkgs { };
  d = builtins.foldl' (a: k: a.${k}) t.pkgs (lp.lib.splitString "." attr);
in lp.stdenvNoCC.mkDerivation {
  name = "fp6-patchcheck-${d.pname or "pkg"}";
  inherit (d) src patches;
  postPatch = d.postPatch or "";
  dontConfigure = true;
  dontBuild = true;
  dontFixup = true;
  installPhase = "cp -r . $out";
}
