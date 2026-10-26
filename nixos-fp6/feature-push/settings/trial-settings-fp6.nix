# Test target: the kernel5 full-features Lomiri GPU system plus the FP6
# lomiri-system-settings panels (Battery, Brightness, System information).
# Build/eval only. Never writes a boot image, slot or partition.
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ../trial-lomiri-full-kernel5-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [
      ./settings-fp6.nix
      ({ config, lib, ... }: {
        hardware.fairphone6.settingsPanels.enable = true;
        system.nixos.tags = [ "settings" ];
        assertions = [{
          assertion = lib.hasInfix "unwrapped-fp6"
            (toString config.system.build.fp6-lomiri-system-settings.buildInputs);
          message = "lomiri-system-settings must be built from the FP6-patched unwrapped package.";
        }];
      })
    ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
