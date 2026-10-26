# Battery and charging (feature push 2026-10-24): charge limit, adaptive
# charging, battery saver, battery health/history/usage, charging info.
# Notes and live results: results/feature-push-20261024/battery/README.md.
#
#  * fp6-batteryd daemon (Rust, system service, root): qcom_battmgr
#    charge_control_{end,start}_threshold (PMIC GLINK charge limit, no NV),
#    adaptive hold/release before the next alarm, battery saver (cpufreq
#    scaling_max_freq caps, powerd com.fairphone6.BrightnessCap, Waydroid
#    container stop, /run/fp6-battery/saver for fp6-app-ready and the XTRA
#    download), and the logger (samples/sessions/usage in
#    /var/lib/fp6-battery, status/history JSON in /run/fp6-battery). Wakes
#    every 60 s plugged in / 300 s on battery, on power_supply uevents
#    (debounced) and display changes. D-Bus com.fairphone6.Battery.
#  * fp6-batteryd agent (user service): GSettings com.fairphone6.battery ->
#    daemon, next alarm from lomiri-indicator-datetime, saver extras
#    (gesture sampling), indicator com.fairphone6.indicator.battery
#    (saver icon; charging speed/time to full, also on the lock screen).
#  * System Settings > Battery: the stock page gains a Loader for the FP6
#    rows; Charge limit, Battery saver, Battery health and Battery usage pages.
#
# Defaults: no charge limit (100 %, the firmware state is not touched until
# a limit is chosen), adaptive charging off, battery saver off and never
# automatic. Nothing suspends; the logger does not assume suspend.
{ config, lib, pkgs, options, ... }:
let
  cfg = config.hardware.fairphone6.battery;
  user = config.services.fp6LomiriSensors.user or "user";
  uid = config.users.users.${user}.uid or 10000;
  gesturesSchema = lib.optional (config.hardware.fairphone6.gestures.enable or false)
    ../gestures/com.fairphone6.gestures.gschema.xml;
  package = pkgs.callPackage ./package.nix {
    inherit (pkgs.lomiri) lomiri-url-dispatcher;
    extraSchemas = gesturesSchema;
  };
  lssPlugin = pkgs.callPackage ./lomiri-system-settings-battery.nix { fp6-batteryd = package; };
  agps = config.services.fp6Agps or { enable = false; };
  xtraUnit = (options.services ? fp6Agps) && agps.enable && agps.xtra.enable && agps.xtra.download;
in {
  options.hardware.fairphone6.battery = {
    enable = lib.mkEnableOption "fp6-batteryd (charge limit, adaptive charging, battery saver, battery history) and the Battery settings pages";
    gpsUnits = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "fp6-gnss-nmea.service" ];
      description = "System units whose running time counts as GPS on in Battery usage.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = config.hardware.fairphone6.settingsPanels.enable or false;
      message = "The FP6 battery pages extend the Battery panel of hardware.fairphone6.settingsPanels.";
    }];

    environment.systemPackages = [ package ];
    environment.pathsToLink = [ "/share/ayatana" ];
    system.build.fp6-battery = { inherit package lssPlugin; };

    # After the other lomiri-system-settings overlays (settings panels,
    # gestures, app-ready): add the plugin, then patch the stock Battery
    # page in the wrapper's merged tree (the unwrapped build is untouched).
    nixpkgs.overlays = lib.mkOrder 2900 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri-system-settings = (lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ lssPlugin ];
        })).overrideAttrs (old: {
          postInstall = (old.postInstall or "") + ''
            page=$out/share/lomiri-system-settings/qml-plugins/battery/PageComponent.qml
            test -e "$page"
            cp --remove-destination "$(readlink -f "$page")" "$page"
            chmod u+w "$page"
            patch "$page" < ${./lss-battery-page-fp6-extras.patch}
            grep -q Fp6BatteryExtras.qml "$page"
            test -e $out/share/lomiri-system-settings/qml-plugins/battery/Fp6BatteryExtras.qml
          '';
        });
      });
    }) ];

    services.dbus.packages = [ (pkgs.writeTextDir "etc/dbus-1/system.d/fp6-batteryd.conf" ''
      <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
        "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
      <busconfig>
        <policy user="root">
          <allow own="com.fairphone6.Battery"/>
          <allow send_destination="com.fairphone6.Battery"/>
        </policy>
        <policy user="${lib.escapeXML user}">
          <allow send_destination="com.fairphone6.Battery"/>
        </policy>
        <policy context="default">
          <deny own="com.fairphone6.Battery"/>
          <deny send_destination="com.fairphone6.Battery"/>
          <allow receive_sender="com.fairphone6.Battery"/>
        </policy>
      </busconfig>
    '') ];

    systemd.services.fp6-batteryd = {
      description = "FP6 battery service (charge limit, adaptive charging, battery saver, history)";
      wantedBy = [ "multi-user.target" ];
      after = [ "dbus.service" "systemd-udev-settle.service" ];
      requires = [ "dbus.service" ];
      path = [ config.systemd.package ];
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.Battery";
        ExecStart = lib.escapeShellArgs ([ (lib.getExe package) "daemon" "--uid" (toString uid) ]
          ++ lib.concatMap (u: [ "--gps-unit" u ]) cfg.gpsUnits);
        Restart = "on-failure";
        RestartSec = 10;
        StateDirectory = "fp6-battery";
        RuntimeDirectory = "fp6-battery";
        RuntimeDirectoryMode = "0755";
        # Keeps the saver flag and the "limit touched" marker across restarts.
        RuntimeDirectoryPreserve = "yes";
        Nice = 10;
        IOSchedulingClass = "idle";
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
        # sysfs stays writable (charge thresholds, cpufreq); nothing else.
        ReadWritePaths = [ "/sys/class/power_supply" "/sys/devices" ];
      };
    };

    # A-GPS XTRA download waits while battery saver is on.
    systemd.services.fp6-agps-xtra = lib.mkIf xtraUnit {
      serviceConfig.ExecCondition = "${pkgs.runtimeShell} -c '! grep -qx 1 /run/fp6-battery/saver 2>/dev/null'";
    };

    systemd.user.services.fp6-batteryd-agent = {
      description = "FP6 battery agent (settings, next alarm, battery saver extras, indicator)";
      wantedBy = [ "graphical-session.target" "ayatana-indicators.target" "lomiri-indicators.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      unitConfig.ConditionUser = user;
      serviceConfig = {
        ExecStart = "${lib.getExe package} agent";
        Restart = "on-failure";
        RestartSec = 5;
        Nice = 5;
      };
    };
  };
}
