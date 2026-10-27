{ config, lib, pkgs, ... }:
let cfg = config.hardware.fairphone6.captions;
in {
  options.hardware.fairphone6.captions = {
    enable = lib.mkEnableOption "private offline live captions";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix {};
      description = "Live captions daemon, CLI and Lomiri application.";
    };
  };
  config = lib.mkIf cfg.enable {
    services.pipewire.enable = true;
    environment.systemPackages = [ cfg.package ];
    systemd.user.services.fp6-captions = {
      description = "Private offline live captions";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "pipewire.service" ];
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.Captions1";
        ExecStart = "${cfg.package}/bin/fp6-captions daemon";
        Restart = "on-failure";
        RestartSec = 2;
        TimeoutStopSec = 3;
        KillMode = "control-group";
        UMask = "0077";
        NoNewPrivileges = true;
        RestrictAddressFamilies = [ "AF_UNIX" ];
        LockPersonality = true;
        RestrictSUIDSGID = true;
        LimitCORE = 0;
        MemoryMax = "600M";
        TasksMax = 64;
        CPUQuota = "200%";
        Nice = 5;
      };
    };
  };
}
