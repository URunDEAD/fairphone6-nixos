{ nixpkgs ? (import ../upstream/mobile-nixos/npins).nixpkgs, binaryArchive ? null }:
let
  pkgs = import nixpkgs { system = "aarch64-linux"; };
  packages = import ./. { inherit pkgs binaryArchive; };
  evaluate = enabled: extra: import (nixpkgs + "/nixos/lib/eval-config.nix") {
    system = "aarch64-linux";
    modules = [ ../waydroid-a810.nix {
      system.stateVersion = "26.05";
      hardware.fairphone6.androidMesaTrial.enable = enabled;
    } extra ];
  };
  off = (evaluate false {}).config;
  on = (evaluate true {}).config;
  stock = (evaluate true { virtualisation.waydroid.enable = true; }).config;
  stockConflictRejected = builtins.any
    (a: pkgs.lib.hasPrefix "Fresh A810 trials" a.message && !a.assertion)
    stock.assertions;
  modulesAreInert = c: !c.virtualisation.waydroid.enable && !c.virtualisation.lxc.enable
    && !(builtins.hasAttr "waydroid-container" c.systemd.services)
    && !(builtins.hasAttr "lxc-net" c.systemd.services)
    && !(builtins.hasAttr "fp6-waydroid-trials" c.systemd.services);
in
assert modulesAreInert off && modulesAreInert on;
assert stockConflictRejected;
assert !(builtins.hasAttr "fp6-waydroid-a810" off.environment.etc);
assert on.environment.etc."fp6-waydroid-a810".source == packages.runtime;
{
  moduleDefaultEnabled = off.hardware.fairphone6.androidMesaTrial.enable;
  enabledModuleStartsServices = false;
  enabledModuleUsesStockWaydroid = on.virtualisation.waydroid.enable;
  enabledModuleUsesGlobalLxc = on.virtualisation.lxc.enable;
  inherit stockConflictRejected;
  enabledModuleExposesOverlay = toString on.environment.etc."fp6-waydroid-a810".source;
  binaryDrv = if binaryArchive == null then null else packages.binary.drvPath;
  sourceDrv = packages.source.drvPath;
  runtimeDrv = packages.runtime.drvPath;
  configureDrv = packages.sourceConfigure.drvPath;
  toolsDrv = packages.tools.drvPath;
  nativeSourceTarget = packages.source.system;
}
