# Separate GPU Phosh target with the same AppArmor runtime-helper repair.
# Parent must validate physical GPU behavior before any activation.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build
}:
let
  base = import ./trial-phosh-kernel-gpu.nix { inherit nixpkgs kernelBuild; };
  evaluated = base.eval.extendModules {
    modules = [ ./apparmor-runtime-helper-native.nix ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
