# The target's patched Lomiri source tree (unpack + every patch + postPatch)
# without this module's edits, for test/default.nix. Same recipe as
# notifications/inline-reply/test/patched-src.nix, with lockSounds off.
# charging-feedback is off too: its postPatch checks an aarch64 store path
# (this tree is unpacked on x86). Both modules only add their instance in
# front of the stock cursorHidingTimer, which each keeps in place.
{ target ? ../../trial-lomiri-full-kernel6-fp6.nix }:
let t = import target { };
    e = t.eval.extendModules { modules = [ ({ lib, ... }: {
          hardware.fairphone6.lockSounds.enable = lib.mkForce false;
          hardware.fairphone6.chargingFeedback.enable = lib.mkForce false; }) ]; };
    l = e.pkgs.lomiri.lomiri;
    pkgs = import (import ../../../upstream/mobile-nixos/npins).nixpkgs { };
    strip = builtins.unsafeDiscardStringContext;
    keepSources = s: let ctx = builtins.getContext s;
                         srcs = pkgs.lib.filterAttrs (n: v: !(v ? outputs) && !(pkgs.lib.hasSuffix ".drv" n)) ctx;
                     in builtins.appendContext (strip s) srcs;
in pkgs.stdenv.mkDerivation {
  name = "lomiri-patched-src";
  src = l.src;
  patches = map (p: if builtins.isPath p then p else /. + strip (toString p)) l.patches;
  postPatch = keepSources l.postPatch;
  dontConfigure = true; dontBuild = true; dontFixup = true;
  installPhase = "cp -r . $out";
}
