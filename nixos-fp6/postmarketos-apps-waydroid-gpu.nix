# All currently reviewed native apps + GPU Waydroid, diagnostic kernel2.
# Build/evaluation does not initialize Android, migrate data, or activate services.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build/gmu-timeout
}:
let
  base = import ./trial-phosh-waydroid.nix { inherit nixpkgs kernelBuild; };
  evaluated = base.eval.extendModules {
    modules = [ ./postmarketos-apps-reviewed-ports.nix {
      hardware.fairphone6.postmarketosApps.enable = true;
    } ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
