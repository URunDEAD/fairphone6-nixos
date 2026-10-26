# Full-features kernel5 Lomiri GPU target plus the Wayland text-input OSK fix
# (osk-wayland-fp6.nix). Build/eval only; never writes a boot image or slot.
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ../trial-lomiri-full-kernel5-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [
      ./osk-wayland-fp6.nix
      {
        hardware.fairphone6.oskWayland.enable = true;
        system.nixos.tags = [ "osk-wayland" ];
      }
    ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
