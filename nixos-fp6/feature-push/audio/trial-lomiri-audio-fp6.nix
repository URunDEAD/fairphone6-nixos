# Lomiri GPU target (../lomiri/trial-lomiri-gpu-fp6.nix) plus the FP6 sound
# feature module. Building never activates anything or writes a partition.
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ../lomiri/trial-lomiri-gpu-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [
      ./audio-fp6-feature.nix
      { system.nixos.tags = [ "audio-feature" ]; }
    ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
