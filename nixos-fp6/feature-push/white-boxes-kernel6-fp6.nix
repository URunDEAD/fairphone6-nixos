# The kernel6 full-features Lomiri target plus white-boxes/white-boxes-fp6.nix.
# A separate entry, so the shared trial files stay untouched. Both candidates
# are switched on here ONLY to build them; the module defaults are false.
# Build/eval only; never activated.
{ nixpkgs ? (import ../upstream/mobile-nixos/npins).nixpkgs
, mesaCcheInvalidateWfi ? true
, gpuPreemptionOff ? false
}:
let
  base = import ./trial-lomiri-full-kernel6-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [ {
      imports = [ ./white-boxes/white-boxes-fp6.nix ];
      hardware.fairphone6.whiteBoxes = { inherit mesaCcheInvalidateWfi gpuPreemptionOff; };
    } ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
}
