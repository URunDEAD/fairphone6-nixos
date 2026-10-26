# kernel5 full Lomiri target + FP6 haptics strength fix (misc/haptics-fp6.nix).
# Build/eval only. Never writes a boot image, slot or partition.
{ nixpkgs ? (import ../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ./trial-lomiri-full-kernel5-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [
      ./misc/haptics-fp6.nix
      { system.nixos.tags = [ "haptics" ]; }
    ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
