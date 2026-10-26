{ config, lib, pkgs, ... }:
let
  cfg = config.services.fp6ManualBrightness;
  package = pkgs.callPackage ./power-brightness-package.nix { };
in {
  options.services.fp6ManualBrightness = {
    enable = lib.mkEnableOption "the FP6 manual-brightness-only Lomiri D-Bus bridge";
    user = lib.mkOption { type = lib.types.str; default = "user"; description = "Local phone user allowed to request manual brightness changes."; };
    uid = lib.mkOption { type = lib.types.ints.positive; default = 10000; description = "UID of the local phone user; checked against D-Bus and logind."; };
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      { assertion = (config.users.users.${cfg.user}.uid or null) == cfg.uid;
        message = "FP6 brightness user and UID must match the configured phone account."; }
    ];
    environment.systemPackages = [ package ];
    system.build.fp6-manual-brightness = package;
    services.dbus.packages = [ (pkgs.writeTextDir "etc/dbus-1/system.d/fp6-manual-brightness.conf" ''
      <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
      <busconfig>
        <policy context="default">
          <deny own="com.lomiri.Repowerd"/>
          <deny own="com.canonical.Unity.Screen"/>
          <deny send_destination="com.lomiri.Repowerd"/>
          <deny send_destination="com.canonical.Unity.Screen"/>
        </policy>
        <policy user="root">
          <allow own="com.lomiri.Repowerd"/>
          <allow own="com.canonical.Unity.Screen"/>
          <allow send_destination="com.lomiri.Repowerd"/>
          <allow send_destination="com.canonical.Unity.Screen"/>
        </policy>
        <policy user="${lib.escapeXML cfg.user}">
          <allow send_destination="com.lomiri.Repowerd"/>
          <allow send_destination="com.canonical.Unity.Screen"/>
        </policy>
      </busconfig>
    '') ];
    # Opt-in backend accepted through the phone's real Lomiri slider. It owns
    # only manual brightness and does not implement power-button/suspend policy.
    systemd.services.fp6-manual-brightness = {
      description = "FP6 manual brightness D-Bus bridge";
      wantedBy = [ "multi-user.target" ];
      after = [ "dbus.service" "systemd-logind.service" ];
      requires = [ "dbus.service" ];
      conflicts = [ "repowerd.service" "powerd.service" ];
      startLimitIntervalSec = 60;
      startLimitBurst = 3;
      serviceConfig = {
        Type = "dbus";
        TimeoutStartSec = 10;
        BusName = "com.lomiri.Repowerd";
        ExecStart = "${package}/bin/fp6-brightness-dbus --uid ${toString cfg.uid}";
        Restart = "on-failure";
        RestartSec = 5;
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ReadOnlyPaths = [ "/sys" ];
        ReadWritePaths = [ "/sys/class/backlight/ae94000.dsi.0/brightness" ];
        CapabilityBoundingSet = "";
        RestrictAddressFamilies = [ "AF_UNIX" ];
        LockPersonality = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        UMask = "0077";
      };
    };
  };
}
