# The kernel6 target's patched Lomiri source tree (unpack + every patch +
# postPatch, which ends with this module's lomiri-memory-predicted.patch at
# mkOrder 4000) for test/default.nix: proves the patch applies in the real
# order; Stage.qml is linted there. With withoutOurs = true the tree is the
# base to regenerate the patch against. Same recipe as
# lock-sounds/test/patched-src.nix; lockSounds and chargingFeedback are off
# (their postPatch checks aarch64 store paths; neither touches Stage.qml).
{ target ? ../../trial-lomiri-full-kernel6-fp6.nix, withoutOurs ? false }:
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
    ourLine = "patch -p1 --no-backup-if-mismatch < ${../lomiri-memory-predicted.patch}";
in pkgs.stdenv.mkDerivation {
  name = "lomiri-patched-src${pkgs.lib.optionalString withoutOurs "-without-memory-predicted"}";
  src = l.src;
  patches = map (p: if builtins.isPath p then p else /. + strip (toString p)) l.patches;
  postPatch = let pp = keepSources l.postPatch; in
    if withoutOurs then builtins.replaceStrings [ ourLine ] [ "" ] pp else pp;
  dontConfigure = true; dontBuild = true; dontFixup = true;
  installPhase = "cp -r . $out";
}
