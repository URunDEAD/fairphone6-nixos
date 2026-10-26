# Build-only test target: the full-features Lomiri GPU target plus
# lomiri-polish-fp6.nix. Never activates anything or writes a phone partition.
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs
, kernelPin ? {}
}:
let
  base = import ../trial-lomiri-full-fp6.nix { inherit nixpkgs kernelPin; };
  evaluated = base.eval.extendModules {
    modules = [
      ./lomiri-polish-fp6.nix
      {
        hardware.fairphone6.lomiriPolish.enable = true;
        system.nixos.tags = [ "lomiri-polish" ];
      }
    ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
