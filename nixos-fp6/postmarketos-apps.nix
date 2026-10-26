# Optional packages only. Does not enable radio, eSIM, account or media services.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.postmarketosApps;
  apps = import ./postmarketos-apps-packages.nix { inherit pkgs; browserPackage = cfg.browserPackage; };
in {
  options.hardware.fairphone6.postmarketosApps = {
    enable = lib.mkEnableOption "the available native pmOS application launchers";
    browserPackage = lib.mkOption {
      type = lib.types.package;
      default = pkgs.firefox-esr;
      description = "Browser replacing the original pmOS Firefox ESR launcher.";
    };
    extraPackages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [];
      description = "Separately reviewed native ports; no insecure-package override is implied.";
    };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = apps.packages ++ cfg.extraPackages;
    assertions = [{
      assertion = !config.networking.modemmanager.enable && !config.services.geoclue2.enable;
      message = "The package-only pmOS migration must retain the reviewed offline modem/location policy.";
    }];
  };
}
