{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build/gmu-timeout
}:
let
  base = import ./camera-fp6-phosh-waydroid.nix { inherit nixpkgs kernelBuild; };
  evaluated = base.eval.extendModules {
    modules = [ ./firefox-mobile.nix {
      hardware.fairphone6.mobileFirefox.enable = true;
    } ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
