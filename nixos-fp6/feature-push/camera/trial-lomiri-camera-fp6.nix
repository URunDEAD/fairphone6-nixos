# Lomiri GPU target (feature-push/lomiri/trial-lomiri-gpu-fp6.nix, y2959 chain:
# kernel4 7.2.0-fp6-nixos4, selfie libcamera) + FP6 lomiri-camera-app stack.
# Build/eval only; never activates or writes a partition.
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ../lomiri/trial-lomiri-gpu-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [
      ./lomiri-camera-fp6.nix
      ({ config, lib, ... }: {
        hardware.fp6LomiriCamera.enable = true;
        system.nixos.tags = [ "camera" ];
      })
    ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
