{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.netpolicy;
  format = pkgs.formats.json {};
  helperConfig = format.generate "fp6-netpolicy.json" {
    inherit (cfg) uid apps;
    system_allow = cfg.systemAllow;
    mobile_adapter = cfg.mobileAdapter;
  };
  policy = pkgs.writeTextDir "share/dbus-1/system.d/org.fairphone.NetPolicy.Helper1.conf" ''
    <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
      "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
      <policy context="default">
        <deny own="org.fairphone.NetPolicy.Helper1"/>
        <deny send_destination="org.fairphone.NetPolicy.Helper1"/>
      </policy>
      <policy user="root"><allow own="org.fairphone.NetPolicy.Helper1"/></policy>
      <policy user="${toString cfg.uid}"><allow send_destination="org.fairphone.NetPolicy.Helper1"/></policy>
    </busconfig>
  '';
in {
  options.hardware.fairphone6.netpolicy = {
    enable = lib.mkEnableOption "Fairphone 6 data saver and smart network policy";
    package = lib.mkOption { type = lib.types.package; default = import ./default.nix { inherit pkgs; }; description = "Network policy package, including Qt 5 Lomiri UI Toolkit."; };
    uid = lib.mkOption { type = lib.types.ints.unsigned; default = 1000; description = "Only this session UID may change policy."; };
    user = lib.mkOption { type = lib.types.str; default = "phablet"; description = "Existing account whose user service runs the daemon."; };
    apps = lib.mkOption {
      type = lib.types.listOf (lib.types.submodule {
        options = {
          id = lib.mkOption { type = lib.types.str; description = "App id supplied by Lomiri focus integration."; };
          unit_pattern = lib.mkOption { type = lib.types.str; description = "Glob for a scope/service basename, e.g. app-org.example.Chat-*.scope."; };
        };
      });
      default = [];
      description = "Explicit app-to-systemd-unit mapping. Ambiguous matches are rejected.";
    };
    systemAllow = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "NetworkManager.service" "systemd-resolved.service" "systemd-timesyncd.service" ];
      description = "System service basenames allowed on costly links. Add actual VPN/push units; updates are restricted by default. Helper probes are always exempt.";
    };
    mobileAdapter = lib.mkOption {
      type = lib.types.nullOr lib.types.str; default = null;
      description = "Absolute root-owned adapter executable implementing status/prefer-mobile/prefer-wifi (DESIGN.md). Null disables automatic mobile routing.";
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      { assertion = cfg.uid >= 1000; message = "netpolicy.uid must be a non-system user"; }
      { assertion = (config.users.users.${cfg.user}.uid or null) == cfg.uid; message = "netpolicy user must exist and have the configured uid"; }
    ];
    networking.networkmanager.enable = true;
    # NetworkManager normally enables ModemManager by default. This target uses
    # oFono; leave radio ownership with it unless the integrator explicitly opts in.
    networking.modemmanager.enable = lib.mkOverride 900 false;
    environment.systemPackages = [ cfg.package ];
    services.dbus.packages = [ policy ];
    boot.kernelModules = [ "nf_tables" "nft_socket" ];
    systemd.services.fp6-netpolicy-helper = {
      description = "Privileged Fairphone network policy helper";
      wantedBy = [ "multi-user.target" ];
      after = [ "dbus.service" "NetworkManager.service" ];
      requires = [ "dbus.service" ];
      serviceConfig = {
        Type = "dbus"; BusName = "org.fairphone.NetPolicy.Helper1";
        ExecStart = "${cfg.package}/bin/netpolicy-helper ${helperConfig}";
        ExecStopPost = "-${cfg.package}/bin/netpolicy-helper --cleanup ${helperConfig}";
        Restart = "on-failure"; RestartSec = 5;
        StateDirectory = "fp6-netpolicy"; StateDirectoryMode = "0700";
        UMask = "0077"; NoNewPrivileges = true;
        ProtectSystem = "strict"; ProtectHome = true; PrivateTmp = true;
        ProtectControlGroups = true; ProtectKernelTunables = true; ProtectKernelModules = true;
        RestrictAddressFamilies = [ "AF_UNIX" "AF_NETLINK" "AF_INET" "AF_INET6" ];
        CapabilityBoundingSet = [ "CAP_NET_ADMIN" "CAP_NET_RAW" ];
        # No PrivateNetwork: nftables must operate on the phone's network namespace.
      };
    };
    systemd.user.services.fp6-netpolicy = {
      description = "Fairphone background data policy coordinator";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session-pre.target" ];
      unitConfig.ConditionUser = cfg.user;
      serviceConfig = {
        Type = "dbus"; BusName = "org.fairphone.NetPolicy1";
        ExecStart = "${cfg.package}/bin/netpolicyd";
        Restart = "on-failure"; RestartSec = 5;
        Slice = "background.slice";
        UMask = "0077"; NoNewPrivileges = true;
      };
    };
  };
}
