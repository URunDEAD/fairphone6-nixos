{ config, lib, pkgs, ... }:
let cfg = config.hardware.fairphone6.share;
in {
  options.hardware.fairphone6.share = {
    enable = lib.mkEnableOption "unified Lomiri system share sheet";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix {};
      description = "Share sheet package.";
    };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    environment.pathsToLink = [ "/share/lomiri-content-hub/peers" ];
    services.dbus.packages = [ cfg.package ];
    xdg.mime.defaultApplications."x-scheme-handler/share" = "com.fairphone6.Share.desktop";
    systemd.user.services.fp6-share = {
      description = "Lomiri unified share sheet";
      environment.PATH = lib.mkForce "/run/current-system/sw/bin";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session-pre.target" ];
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.Share1";
        ExecStart = "${cfg.package}/bin/fp6-share daemon";
        Restart = "on-failure";
        RestartSec = 2;
        RuntimeDirectory = "fp6-share";
        RuntimeDirectoryMode = "0700";
        UMask = "0077";
        NoNewPrivileges = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        TimeoutStopSec = 5;
      };
    };
  };
}
