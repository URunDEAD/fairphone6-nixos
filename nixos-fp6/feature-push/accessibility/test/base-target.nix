# The kernel6 full target with this module switched off: the base tree for
# test/mkpatch.sh (nix-build ../spread-close-all/test/patched-src.nix
#   --arg target ./test/base-target.nix -o $SCRATCH/psrc-base).
{ }:
let t = import ../../trial-lomiri-full-kernel6-fp6.nix { };
    e = t.eval.extendModules { modules = [ ({ lib, ... }: { hardware.fairphone6.accessibility.enable = lib.mkForce false; }) ]; };
in { inherit (e) pkgs config; }
