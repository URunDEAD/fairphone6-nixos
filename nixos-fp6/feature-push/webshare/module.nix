{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.webshare;
in {
  options.hardware.fairphone6.webshare = {
    enable = lib.mkEnableOption "temporary Lomiri HTTP sharing on Wi-Fi/hotspot";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix {};
      description = "WebShare package to install.";
    };
    openFirewallInterfaces = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [];
      example = [ "wlan0" "ap0" ];
      description = "Explicit Wi-Fi/hotspot interfaces on which to allow TCP 8765. Never opens the global firewall.";
    };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    # Content-hub reads peers from /run/current-system/sw, not a desktop key.
    environment.pathsToLink = [ "/share/lomiri-content-hub/peers" ];
    networking.firewall.interfaces = lib.genAttrs cfg.openFirewallInterfaces (_: { allowedTCPPorts = [ 8765 ]; });
    services.dbus.packages = [ cfg.package ];
    systemd.user.services.fp6-webshare = {
      description = "Temporary local-network WebShare";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session-pre.target" ];
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.WebShare1";
        ExecStart = "${cfg.package}/bin/fp6-webshare daemon";
        Restart = "on-failure";
        RestartSec = 2;
        RuntimeDirectory = "fp6-webshare";
        RuntimeDirectoryMode = "0700";
        UMask = "0077";
        NoNewPrivileges = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_NETLINK" ];
        TimeoutStopSec = 5;
      };
    };
  };
}
