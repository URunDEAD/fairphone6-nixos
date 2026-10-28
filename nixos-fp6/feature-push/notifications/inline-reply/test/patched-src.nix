# The target's patched Lomiri source tree (unpack + every patch + postPatch)
# without this module's patch, for run-tests.sh and mkpatch.sh. Same recipe
# as spread-close-all/test/patched-src.nix, with inlineReply switched off.
#   nix-build test/patched-src.nix -o $SCRATCH/psrc
{ target ? ../../../trial-lomiri-full-kernel6-fp6.nix }:
let t = import target { };
    e = t.eval.extendModules { modules = [ ({ lib, ... }: { hardware.fairphone6.inlineReply.enable = lib.mkForce false; }) ]; };
    l = e.pkgs.lomiri.lomiri;
    pkgs = import (import ../../../../upstream/mobile-nixos/npins).nixpkgs { };
    strip = builtins.unsafeDiscardStringContext;
    # Keep the source files (patches) referenced by postPatch, not the aarch64 builds.
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
