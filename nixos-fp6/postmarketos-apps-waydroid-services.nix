# Separate all26 native apps + persistent Waydroid + app runtime services.
# Build/evaluation never initializes Android, configures accounts or adds remotes.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build/gmu-timeout
}:
let
  base = import ./postmarketos-apps-waydroid-gpu.nix { inherit nixpkgs kernelBuild; };
  evaluated = base.eval.extendModules {
    modules = [ ./postmarketos-apps-services.nix {
      hardware.fairphone6.postmarketosApps.services.enable = true;
    } ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
