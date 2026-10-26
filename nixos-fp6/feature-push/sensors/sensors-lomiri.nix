# FP6 sensors & input integration for Lomiri (feature push 2026-10-24).
#
# Import this module INSTEAD of enabling services.fp6ManualBrightness (both own
# com.lomiri.Repowerd). It:
#   * keeps hardware.fp6IioSensors (iio-sensor-proxy before the display manager,
#     no libssc/fastrpc) and adds the postmarketOS fp6-device-tweaks rule that
#     forces the ICM-42631 accelerometer onto iio-sensor-proxy's POLL driver
#     (inv_icm42600 has no IIO trigger; the buffer driver registers a NULL
#     device, leaves the kfifo enabled -> raw reads EBUSY -> no orientation);
#   * pins QtSensors' QOrientationSensor to the iio-sensor-proxy backend;
#   * applies the tested qtmir orientation owner-thread fix (not yet in the
#     production overlay, results/qtmir-orientation-thread/);
#   * runs fp6-lomiri-powerd as com.lomiri.Repowerd + com.canonical.Unity.Screen
#     (manual + automatic brightness, call proximity blanking, double-tap to
#     wake on top of kernel patch 0003, optional screen power);
#   * makes sure hfd-service (Lomiri haptics) starts at boot.
{ config, lib, pkgs, ... }:
let
  cfg = config.services.fp6LomiriSensors;
  package = pkgs.callPackage ./package.nix { };
  touchSysfs = "/sys/devices/platform/soc@0/ac0000.geniqup/a80000.spi/spi_master/spi0/spi0.0";
  backlight = "/sys/class/backlight/ae94000.dsi.0";
  curveArg = lib.concatMapStringsSep "," (p: "${toString (builtins.elemAt p 0)}:${toString (builtins.elemAt p 1)}") cfg.autoBrightnessCurve;
  sensorsConf = pkgs.writeTextDir "etc/xdg/QtProject/Sensors.conf" ''
    [Default]
    QOrientationSensor=iio-sensor-proxy.orientationsensor
    QLightSensor=iio-sensor-proxy.lightsensor
  '';
in {
  imports = [ ../../sensors-fp6.nix ];

  options.services.fp6LomiriSensors = {
    enable = lib.mkEnableOption "FP6 sensors/input integration for Lomiri";
    user = lib.mkOption { type = lib.types.str; default = "user"; };
    uid = lib.mkOption { type = lib.types.ints.positive; default = 10000; };
    screenPower = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Implement Unity.Screen setScreenPowerMode (power key / notifications)
        by backlight 0 + touch gesture-LP. Leave off if another component owns
        display power. Proximity blanking works either way.
      '';
    };
    doubleTapToWake = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Arm double-tap-to-wake at startup (needs the ESWIN lp_mode kernel patch).";
    };
    autoBrightnessCurve = lib.mkOption {
      type = lib.types.listOf (lib.types.listOf lib.types.number);
      default = [ [0 12] [5 30] [20 55] [80 90] [300 135] [1000 180] [5000 235] [20000 255] ];
      description = "[lux logical(1..255)] points; STK36C61 lux uses the uncalibrated legacy STK3310 scale.";
    };
    qtmirOrientationFix = lib.mkOption { type = lib.types.bool; default = true; };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      { assertion = !(config.services.fp6ManualBrightness.enable or false);
        message = "fp6LomiriSensors supersedes fp6ManualBrightness; enable only one (both own com.lomiri.Repowerd)."; }
      { assertion = (config.users.users.${cfg.user}.uid or null) == cfg.uid;
        message = "fp6LomiriSensors user and UID must match the configured phone account."; }
    ];

    hardware.fp6IioSensors.enable = true;

    # Must sort after 80-iio-sensor-proxy.rules (same as pmOS 81-fp6-accel-poll.rules).
    services.udev.packages = [ (pkgs.writeTextDir "lib/udev/rules.d/81-fp6-accel-poll.rules" ''
      SUBSYSTEM=="iio", ATTR{name}=="icm426*-accel", ENV{IIO_SENSOR_PROXY_TYPE}="iio-poll-accel"
    '') ];

    environment.etc."xdg/QtProject/Sensors.conf".source = "${sensorsConf}/etc/xdg/QtProject/Sensors.conf";
    environment.systemPackages = [ package sensorsConf ];
    system.build.fp6-lomiri-powerd = package;

    nixpkgs.overlays = lib.mkIf cfg.qtmirOrientationFix (lib.mkAfter [ (final: prev: {
      lomiri = prev.lomiri.overrideScope (lfinal: lprev: {
        qtmir = lprev.qtmir.overrideAttrs (old: {
          patches = (old.patches or []) ++ [ ../../qtmir-orientation-owner-thread.patch ];
        });
      });
    }) ]);

    # hfd-service ships a unit through systemd.packages (lomiri module) but no
    # WantedBy; start it at boot so haptics work before the first D-Bus call.
    systemd.services.hfd-service.wantedBy = [ "multi-user.target" ];

    services.dbus.packages = [ (pkgs.writeTextDir "etc/dbus-1/system.d/fp6-lomiri-powerd.conf" ''
      <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
      <busconfig>
        <policy context="default">
          <deny own="com.lomiri.Repowerd"/>
          <deny own="com.canonical.Unity.Screen"/>
          <deny send_destination="com.lomiri.Repowerd"/>
          <deny send_destination="com.canonical.Unity.Screen"/>
          <!-- signals/introspection/reads for every session user -->
          <allow receive_sender="com.canonical.Unity.Screen"/>
          <allow receive_sender="com.lomiri.Repowerd"/>
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
        <!-- LightDM greeter (Lomiri greeter mode also loads the Powerd plugin) -->
        <policy user="lightdm">
          <allow send_destination="com.canonical.Unity.Screen"/>
        </policy>
      </busconfig>
    '') ];

    systemd.services.fp6-lomiri-powerd = {
      description = "FP6 Lomiri display backend (brightness, auto-brightness, proximity, double-tap)";
      wantedBy = [ "multi-user.target" ];
      before = [ "display-manager.service" ];
      after = [ "dbus.service" "systemd-logind.service" "iio-sensor-proxy.service" "systemd-udev-settle.service" ];
      wants = [ "iio-sensor-proxy.service" ];
      requires = [ "dbus.service" ];
      conflicts = [ "repowerd.service" "powerd.service" "fp6-manual-brightness.service" ];
      startLimitIntervalSec = 60;
      startLimitBurst = 3;
      serviceConfig = {
        Type = "dbus";
        BusName = "com.lomiri.Repowerd";
        TimeoutStartSec = 15;
        ExecStart = lib.concatStringsSep " " ([
          "${package}/bin/fp6-lomiri-powerd" "--uid" (toString cfg.uid) "--curve" curveArg
        ] ++ lib.optional cfg.screenPower "--screen-power"
          ++ lib.optional cfg.doubleTapToWake "--double-tap-default");
        Restart = "on-failure";
        RestartSec = 5;
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        # Needs read access to the ESWIN evdev node for KEY_WAKEUP only.
        DevicePolicy = "closed";
        DeviceAllow = [ "char-input r" ];
        ReadOnlyPaths = [ "/sys" ];
        ReadWritePaths = [
          "${backlight}/brightness"
          "-${touchSysfs}/gesture_wakeup"
          "-${touchSysfs}/lp_mode"
        ];
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
