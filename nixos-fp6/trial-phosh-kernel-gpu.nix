# Separate GPU-rendered Phosh userspace on the prepared kernel. Building this
# target never activates it; first use requires clean-boot GPU acceptance.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build
}:
let
  base = import ./trial-phosh-kernel.nix { inherit nixpkgs kernelBuild; };
  evaluated = base.eval.extendModules {
    modules = [{ hardware.fairphone6.phosh.softwareRendering = false; }];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
