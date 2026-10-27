# Mir upgrade prototype: the kernel6 full target (trial-lomiri-full-kernel6-fp6.nix)
# with hardware.fairphone6.mirUpgrade.enable = true (qtmir + Lomiri on
# nixpkgs' Mir 2.28, ext-data-control for fp6-privacy-clipboard only).
# The default kernel6 target is unchanged. See mir-upgrade/mir-upgrade-fp6.nix
# and results/feature-push-20261024/mir-upgrade/README.md.
# Build/eval only. Never writes a boot image, slot or partition.
#   ... build --file trial-lomiri-full-kernel6-mir228-fp6.nix qtmir|lomiri|lomiri-system-settings|toplevel
{ nixpkgs ? (import ../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ./trial-lomiri-full-kernel6-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [ {
      hardware.fairphone6.mirUpgrade.enable = true;
      system.nixos.tags = [ "mir228" ];
    } ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
  inherit (evaluated.pkgs) mir;
  inherit (evaluated.pkgs.lomiri) qtmir lomiri lomiri-system-settings;
  toplevel = evaluated.config.system.build.toplevel;
}
