# The kernel6 full-features Lomiri target with
# hardware.fairphone6.modem.persistShadow = true (persistent read-only EFS
# shadow for rmtfs, cellular/rmtfs-shadow/rmtfs-shadow-fp6.nix).
# A separate entry for the supervised RAM-boot test, so the shared trial
# files (and the module default, false) stay untouched.
# Build/eval only. Never writes a boot image, slot or partition.
{ nixpkgs ? (import ../upstream/mobile-nixos/npins).nixpkgs }:
let
  base = import ./trial-lomiri-full-kernel6-fp6.nix { inherit nixpkgs; };
  evaluated = base.eval.extendModules {
    modules = [ { hardware.fairphone6.modem.persistShadow = true; } ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
