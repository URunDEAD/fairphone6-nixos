{ config, lib, pkgs, ... }:
let
  cfg = config.services.fp6Mms;
in {
  options.services.fp6Mms = {
    enable = lib.mkEnableOption "Nuntium MMS integration with an already validated oFono backend";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./nuntium-package.nix { };
      description = "Native pinned Nuntium; not a modem backend, SIM account provider or MMS application.";
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = config.services.desktopManager.lomiri.enable;
      message = "FP6 MMS integration expects Lomiri's download/upload and Telepathy services.";
    }];
    environment.systemPackages = [ cfg.package ];
    system.build.fp6-nuntium = cfg.package;
    systemd.user.services.nuntium = {
      description = "MMS between oFono and Telepathy using Lomiri transfer services";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      unitConfig = {
        StartLimitIntervalSec = 60;
        StartLimitBurst = 3;
      };
      serviceConfig = {
        Type = "dbus";
        BusName = "org.ofono.mms";
        ExecStart = lib.getExe cfg.package;
        Restart = "on-failure";
        RestartSec = 10;
      };
    };
    # Explicit enablement can register an oFono push agent and activate an
    # MMS data context. Do not enable until those backend contracts/carrier
    # configuration are validated. No modem owner or account is created here.
  };
}
