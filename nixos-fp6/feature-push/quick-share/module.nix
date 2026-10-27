{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.quickShare;
  args = lib.escapeShellArgs ([
    "--device-name" cfg.deviceName "--port" (toString cfg.port)
    "--interfaces" (lib.concatStringsSep "," cfg.wifiInterfaces)
    "--max-bytes" (toString cfg.maxBytes)
  ]);
in {
  options.hardware.fairphone6.quickShare = {
    enable = lib.mkEnableOption "Quick Share LAN sharing with a Lomiri app";
    package = lib.mkOption { type = lib.types.package; default = pkgs.callPackage ./package.nix {}; description = "Quick Share package."; };
    deviceName = lib.mkOption { type = lib.types.str; default = "Fairphone 6"; description = "Public device name while visible."; };
    wifiInterfaces = lib.mkOption {
      type = lib.types.listOf (lib.types.strMatching "wl[a-zA-Z0-9_.-]+");
      default = [ "wlan0" ];
      description = "Explicit Wi-Fi interface allowlist for mDNS, TCP and firewall rules. Use physical Wi-Fi interfaces with wl names; never Ethernet or wildcard names.";
    };
    port = lib.mkOption { type = lib.types.port; default = 53317; description = "Single fixed inbound TCP port; no dynamic port range is needed."; };
    maxBytes = lib.mkOption { type = lib.types.ints.positive; default = 1073741824; description = "Maximum total received bytes per transfer."; };
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      { assertion = cfg.wifiInterfaces != []; message = "Quick Share requires explicit Wi-Fi interfaces."; }
      { assertion = cfg.port >= 1024; message = "Quick Share user service requires an unprivileged TCP port."; }
      { assertion = builtins.stringLength cfg.deviceName > 0 && builtins.stringLength cfg.deviceName <= 64; message = "Quick Share deviceName must be 1–64 UTF-8 bytes."; }
    ];
    environment.systemPackages = [ cfg.package ];
    services.dbus.packages = [ cfg.package ];
    # rqs uses its own mDNS sockets. Avahi provides system .local resolution;
    # do not enable its global firewall rule or advertise other services here.
    services.avahi = {
      enable = true;
      nssmdns4 = true;
      openFirewall = false;
      allowInterfaces = cfg.wifiInterfaces;
    };
    networking.firewall.interfaces = lib.genAttrs cfg.wifiInterfaces (_: {
      allowedTCPPorts = [ cfg.port ];
      allowedUDPPorts = [ 5353 ];
    });
    systemd.user.services.fp6-quickshare = {
      description = "Fairphone 6 Quick Share LAN service";
      wantedBy = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      path = [ cfg.package ];
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.QuickShare1";
        ExecStart = "${cfg.package}/bin/fp6-quickshare-daemon ${args}";
        Restart = "on-failure";
        UMask = "0077";
        NoNewPrivileges = true;
        RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" "AF_NETLINK" ];
      };
    };
  };
}
