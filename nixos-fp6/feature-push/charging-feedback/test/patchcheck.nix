# Runs only unpack/patch/postPatch of the target's (aarch64) lomiri on the
# laptop and keeps the patched tree: checks that every patch layer, this
# module's included, applies, and gives test/preview.sh the patched QML.
# Modules whose postPatch needs aarch64-only builds (lock-sounds' schema and
# sound bundle) can be switched off for this check with `disable`.
#   nix-build test/patchcheck.nix -o /tmp/fp6-lomiri-cf
{ target ? ../../trial-lomiri-full-kernel6-fp6.nix
, disable ? [ "lockSounds" ] }:
let
  t = import target { };
  lp = import (import ../../../upstream/mobile-nixos/npins).nixpkgs { };
  e = t.eval.extendModules {
    modules = map (n: { hardware.fairphone6.${n}.enable = lp.lib.mkForce false; })
      (builtins.filter (n: t.options.hardware.fairphone6 ? ${n}) disable);
  };
  d = e.pkgs.lomiri.lomiri;
in lp.stdenvNoCC.mkDerivation {
  name = "fp6-patchcheck-charging-feedback-lomiri";
  inherit (d) src patches;
  postPatch = d.postPatch or "";
  dontConfigure = true;
  dontBuild = true;
  dontFixup = true;
  installPhase = "cp -r . $out";
}
