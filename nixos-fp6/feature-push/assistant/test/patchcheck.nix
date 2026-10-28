# Runs only unpack/patch/postPatch of the target's (aarch64) lomiri on the
# laptop and keeps the patched tree: every module's Lomiri edits, this one's
# (drawer "Ask the assistant" row, quick-settings tile; mkOrder 4300) last.
# lock-sounds' postPatch needs aarch64 builds, so it is off for this check.
#   nix-build test/patchcheck.nix -o /tmp/fp6-lomiri-assistant
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
  name = "fp6-patchcheck-assistant-lomiri";
  inherit (d) src patches;
  postPatch = d.postPatch or "";
  dontConfigure = true;
  dontBuild = true;
  dontFixup = true;
  installPhase = "cp -r . $out";
}
