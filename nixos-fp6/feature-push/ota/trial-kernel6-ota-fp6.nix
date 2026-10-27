# The kernel6 full target plus OTA updates (update service on, slot
# switching OFF): what trial-lomiri-full-kernel6-fp6.nix becomes once the
# ota import is added there. Build/eval only. Never writes a partition.
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ../trial-lomiri-full-kernel6-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [ ({ ... }: {
      imports = [ ./ota-fp6.nix ];
      hardware.fairphone6.ota.enable = true;
      hardware.fairphone6.ota.allowSlotSwitch = false;
    }) ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
