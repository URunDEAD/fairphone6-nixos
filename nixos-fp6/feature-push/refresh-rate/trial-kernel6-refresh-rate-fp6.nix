# Evaluation/build target: the kernel6 full target with the refresh-rate
# feature ENABLED (hardware.fairphone6.refreshRate.enable = true). Same
# kernel6 (one 120 Hz mode, no panel_rate_hz), so on a phone this changes
# nothing visible except the Settings note; it is for evaluating the
# Lomiri/Settings overlays. The supervised test pairs it with a kernel7
# candidate kernel (0009/0010), RAM-booted only.
# Build/eval only. Never writes a boot image, slot or partition.
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ../trial-lomiri-full-kernel6-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [ ({ lib, ... }: { hardware.fairphone6.refreshRate.enable = lib.mkForce true; }) ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
