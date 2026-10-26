# Build-only test target: the full-features kernel5 Lomiri GPU target plus
# lomiri-misc-fp6.nix. Never activates anything or writes a phone partition.
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ../trial-lomiri-full-kernel5-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [
      ./lomiri-misc-fp6.nix
      {
        hardware.fairphone6.lomiriMisc.enable = true;
        system.nixos.tags = [ "lomiri-misc" ];
      }
    ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
