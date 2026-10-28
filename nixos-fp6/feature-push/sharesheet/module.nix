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
    # lomiri-content-hub-service aborted (SIGABRT, 15x in one day on r4) when a
    # peer file listed a type its schema lacks: share: ["all"] reaches
    # QGSettings::get("all") on com.lomiri.content.hub.share, which has no such
    # key upstream. The peers are fixed (kdeconnect, webshare); this patch makes
    # every registry lookup check the schema first, so a bad third-party peer
    # file only logs a warning.
    nixpkgs.overlays = [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri-content-hub = lprev.lomiri-content-hub.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-content-hub-tolerant-registry.patch ];
          passthru = (old.passthru or { }) // { fp6TolerantRegistry = true; };
        });
      });
    }) ];
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
