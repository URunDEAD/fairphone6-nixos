{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.dictation;
in {
  options.hardware.fairphone6.dictation = {
    enable = lib.mkEnableOption "offline Maliit voice dictation";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix {};
      description = "Dictation daemon, CLI and Lomiri Settings package.";
    };
  };
  config = lib.mkIf cfg.enable {
    services.pipewire.enable = true;
    environment.systemPackages = [ cfg.package ];
    systemd.user.services.fp6-dictation = {
      description = "Private offline voice dictation";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "pipewire.service" ];
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.Dictation1";
        ExecStart = "${cfg.package}/bin/fp6-dictationd";
        Restart = "on-failure";
        RestartSec = 2;
        TimeoutStopSec = 5;
        KillMode = "control-group";
        UMask = "0077";
        NoNewPrivileges = true;
        RestrictAddressFamilies = [ "AF_UNIX" ];
        LockPersonality = true;
        RestrictSUIDSGID = true;
        # All audio stays in bounded memory. Avoid core dumps containing speech.
        LimitCORE = 0;
        MemoryMax = "500M";
        TasksMax = 64;
      };
    };
  };
}
