# trial-lomiri-full-kernel5-fp6.nix plus the Waydroid display/suspend fix
# (misc/waydroid-lomiri-fp6.nix): Android at 1116x2412 physical (372x804
# logical at scale 3), 480 dpi, no auto-freeze. Build/eval only.
{ nixpkgs ? (import ../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ./trial-lomiri-full-kernel5-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [
      ./misc/waydroid-lomiri-fp6.nix
      { hardware.fairphone6.waydroidLomiri.enable = true; }
    ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
