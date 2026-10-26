# Separate Phosh CPU userspace on the prepared kernel. Build only; never boots
# or activates itself. GPU acceleration needs its own clean-boot acceptance.
{ nixpkgs ? (import ./upstream/mobile-nixos/npins).nixpkgs
, kernelBuild ? ../kernel-build
}:
let
  base = import ./trial-kernel-ui.nix { inherit nixpkgs kernelBuild; };
  evaluated = base.eval.extendModules {
    modules = [
      ./phosh-fp6.nix
      ({ lib, ... }: {
        # Phosh uses its own settings backend instead of the Lomiri bridge.
        services.fp6ManualBrightness.enable = lib.mkForce false;
      })
    ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
