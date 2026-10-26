# Separate rear-camera userspace integration candidate. Build only; no capture.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build/gmu-timeout
}:
let
  base = import ./postmarketos-apps-waydroid-services.nix { inherit nixpkgs kernelBuild; };
  evaluated = base.eval.extendModules {
    modules = [ ./camera-fp6.nix {
      hardware.fp6Camera = {
        enable = true;
        enablePipeWire = true;
        enableSnapshot = true;
      };
    } ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
