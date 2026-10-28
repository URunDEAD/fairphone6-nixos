# Runs only unpack/patch/postPatch of the target's (aarch64) Lomiri on the
# laptop and keeps the patched tree (as charging-feedback/test/patchcheck.nix):
# every Lomiri layer including this module's (mkOrder 4200) applies; the
# preview takes Fp6PermissionPrompt.qml and Shell.qml from it.
#   nix-build test/patchcheck.nix -o /tmp/fp6-lomiri-perm
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
  name = "fp6-patchcheck-app-permissions-lomiri";
  inherit (d) src patches;
  postPatch = d.postPatch or "";
  dontConfigure = true;
  dontBuild = true;
  dontFixup = true;
  installPhase = "cp -r . $out";
}
