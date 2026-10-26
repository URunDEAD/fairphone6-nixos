# Separate persistent Waydroid target; evaluating/building never starts Android.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build/gmu-timeout
}:
let
  base = import ./trial-phosh-kernel-gpu-gmu-timeout.nix { inherit nixpkgs kernelBuild; };
  evaluated = base.eval.extendModules {
    modules = [ ./waydroid-fp6-production.nix {
      hardware.fairphone6.waydroidGPU.enable = true;
    } ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
