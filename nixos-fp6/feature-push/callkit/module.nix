{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.calls;
in {
  options.hardware.fairphone6.calls = {
    enable = lib.mkEnableOption "the experimental Calls1 session broker";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix {};
      description = "Call broker package including the synthetic QML preview.";
    };
    sessionTarget = lib.mkOption {
      type = lib.types.str;
      default = "graphical-session.target";
      example = "lomiri-session.target";
      description = "Target actually started by the phone's Lomiri session.";
    };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    systemd.user.services.fp6-calls = {
      description = "Fairphone internet call broker";
      wantedBy = [ cfg.sessionTarget ];
      partOf = [ cfg.sessionTarget ];
      after = [ "graphical-session-pre.target" ];
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.Calls1";
        ExecStart = "${cfg.package}/bin/fp6-callsd";
        Restart = "on-failure";
        RestartSec = 2;
        TimeoutStopSec = 5;
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        RestrictAddressFamilies = [ "AF_UNIX" ];
        UMask = "0077";
        LockPersonality = true;
      };
    };
  };
}
