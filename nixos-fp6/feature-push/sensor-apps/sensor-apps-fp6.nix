# FP6 sensor apps for Lomiri (batch 1): Compass, Level, Measure, Altimeter,
# Sound meter, Light meter, GPS status, Sensors (dashboard).
#
#  * One package (package.nix): Rust backend (std only) + a one-class Qt 5.15
#    QML shim + Lomiri UI Toolkit QML. One launcher binary
#    (fp6-sensor-tools TOOL); each app has its own desktop entry fp6-TOOL
#    with X-Lomiri-Touch=true and a real PNG icon. Extra desktop keys (the
#    drawer folder) come from one option, desktopExtraKeys.
#  * Sensors are read from IIO sysfs in poll mode, read-only: no sampling
#    frequency, buffer or trigger is changed, so iio-sensor-proxy is not
#    affected. Feeds run only while the app is the active (visible) app.
#  * Satellites: GeoClue exposes none, and the modem NMEA socket
#    (/run/fp6-gnss, cellular/) is geoclue-only. fp6-gnss-skyfeed is a
#    socket-activated, per-connection service (DynamicUser + group geoclue)
#    that reads that socket while a GPS status app is connected and passes on
#    only satellites (GSV) and fix quality / satellites used / HDOP. No
#    position leaves it; position comes from GeoClue (agent-authorized).
#    Nothing in cellular/ is changed.
#  * Background pressure log: user timer installed, but its service only
#    runs when the user turned it on in the Altimeter (marker file). Off by
#    default.
# See results/feature-push-20261024/sensor-apps/README.md.
{ config, lib, pkgs, options, ... }:
let
  cfg = config.hardware.fairphone6.sensorApps;
  haveFolders = options.hardware.fairphone6 ? drawerFolders;

  pkg = pkgs.callPackage ./package.nix {
    pipewire = config.services.pipewire.package;
  };

  appMeta = {
    compass = {
      name = "Compass";
      comment = "Tilt-compensated compass with true north";
      keywords = "compass;heading;north;magnetometer;";
      portrait = true;
    };
    level = {
      name = "Level";
      comment = "Bubble level, inclinometer and protractor";
      keywords = "level;spirit;bubble;inclinometer;protractor;angle;";
      portrait = true;
    };
    measure = {
      name = "Measure";
      comment = "Ruler and height/distance by phone tilt";
      keywords = "ruler;measure;length;height;distance;";
      portrait = true;
    };
    altimeter = {
      name = "Altimeter";
      comment = "Barometer, altitude and pressure trend";
      keywords = "altimeter;barometer;pressure;altitude;weather;";
      portrait = false;
    };
    sound = {
      name = "Sound Meter";
      comment = "Sound level (dB) from the microphone";
      keywords = "sound;noise;decibel;dB;meter;";
      portrait = false;
    };
    light = {
      name = "Light Meter";
      comment = "Lux and photographic exposure";
      keywords = "light;lux;exposure;EV;photo;";
      portrait = false;
    };
    gps = {
      name = "GPS Status";
      comment = "Satellites, fix and coordinates";
      keywords = "gps;gnss;satellites;location;coordinates;";
      portrait = false;
    };
    dashboard = {
      name = "Sensors";
      comment = "Live values of every sensor";
      keywords = "sensors;accelerometer;gyroscope;magnetometer;proximity;";
      portrait = false;
    };
  };

  enabledApps = lib.filterAttrs (n: _: cfg.apps.${n}.enable) appMeta;

  desktopFile = tool: m: ''
    [Desktop Entry]
    Type=Application
    Name=${m.name}
    Comment=${m.comment}
    Keywords=${m.keywords}
    Exec=${pkg}/bin/fp6-sensor-tools ${tool}
    Icon=${pkg}/share/fp6-sensor-apps/icons/fp6-${tool}.png
    Terminal=false
    Categories=Utility;
    X-Lomiri-Touch=true
    X-Lomiri-Default-Department-ID=accessories
  '' + lib.optionalString m.portrait ''
    X-Lomiri-Supported-Orientations=portrait
  '' + lib.concatStrings (lib.mapAttrsToList (k: v: "${k}=${v}\n") cfg.desktopExtraKeys);

  desktopEntries = pkgs.runCommand "fp6-sensor-apps-desktop" { } (''
    mkdir -p $out/share/applications
  '' + lib.concatStrings (lib.mapAttrsToList (tool: m: ''
    cat > $out/share/applications/fp6-${tool}.desktop <<'EOF'
    ${desktopFile tool m}EOF
    test "$(grep -c '^X-Lomiri-Touch=true$' $out/share/applications/fp6-${tool}.desktop)" = 1
    test -f ${pkg}/share/fp6-sensor-apps/icons/fp6-${tool}.png
  '') enabledApps));
in
{
  options.hardware.fairphone6.sensorApps = {
    enable = lib.mkEnableOption "the FP6 sensor apps (compass, level, measure, altimeter, sound/light meter, GPS status, sensor dashboard)";

    apps = lib.mapAttrs (tool: m: {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Install the ${m.name} launcher (fp6-${tool}.desktop).";
      };
    }) appMeta;

    desktopExtraKeys = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { "X-Lomiri-Folder" = "Tools"; };
      description = ''
        Extra keys added to every sensor-app desktop entry (e.g. the app
        drawer folder). Suggested folder icon:
        ''${package}/share/icons/hicolor/scalable/apps/fp6-tools-folder.svg
        (icon name fp6-tools-folder).
      '';
    };

    satelliteFeed = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = cfg.apps.gps.enable && config.services.geoclue2.enable;
        defaultText = lib.literalExpression "apps.gps.enable && services.geoclue2.enable";
        description = ''
          fp6-gnss-skyfeed: socket-activated read-only satellite view (GSV,
          fix quality, satellites used, HDOP; no position) for the GPS
          status app. Reads GeoClue's NMEA socket only while a client is
          connected and location is on.
        '';
      };
      socketGroup = lib.mkOption {
        type = lib.types.str;
        default = "radio";
        description = "Group allowed to connect to /run/fp6-gnss-sky/sky.sock.";
      };
      nmeaSocket = lib.mkOption {
        type = lib.types.str;
        default = "/run/fp6-gnss/nmea.sock";
        description = "GeoClue NMEA socket (services.fp6Cellular GNSS).";
      };
    };

    apparmor = lib.mkOption {
      type = lib.types.enum [ "disable" "complain" "enforce" ];
      default = if config.hardware.fairphone6.apparmor.enable or false then "complain" else "disable";
      defaultText = lib.literalExpression ''"complain" when hardware.fairphone6.apparmor.enable'';
      description = ''
        Mode of the fp6-sensor-apps AppArmor profile (uses the abstractions
        of ../apparmor). Complain until it has been exercised live.
      '';
    };

    barometerLogger = lib.mkOption {
      type = lib.types.bool;
      default = cfg.apps.altimeter.enable;
      description = ''
        Install the opt-in background pressure logger (user timer every
        15 min). It does nothing until the user turns it on in the
        Altimeter's settings.
      '';
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      environment.systemPackages = [ pkg desktopEntries ];
      system.build.fp6-sensor-apps = pkg;
      system.build.fp6-sensor-apps-desktop = desktopEntries;
    }

    # Toolbox icon for the drawer folder the entries ask for (only if the
    # drawer-folders module is imported; another agent's module owns it).
    (lib.optionalAttrs haveFolders (lib.mkIf (cfg.desktopExtraKeys ? "X-Lomiri-Folder") {
      hardware.fairphone6.drawerFolders.${cfg.desktopExtraKeys."X-Lomiri-Folder"}.icon =
        lib.mkDefault "${pkg}/share/icons/hicolor/scalable/apps/fp6-tools-folder.svg";
    }))

    (lib.mkIf cfg.satelliteFeed.enable {
      systemd.sockets.fp6-gnss-skyfeed = {
        description = "Read-only GNSS satellite feed for the GPS status app";
        wantedBy = [ "sockets.target" ];
        listenStreams = [ "/run/fp6-gnss-sky/sky.sock" ];
        socketConfig = {
          Accept = true;
          MaxConnections = 4;
          SocketUser = "root";
          SocketGroup = cfg.satelliteFeed.socketGroup;
          SocketMode = "0660";
          DirectoryMode = "0755";
        };
      };
      systemd.services."fp6-gnss-skyfeed@" = {
        description = "GNSS satellite feed (one client)";
        serviceConfig = {
          ExecStart = pkg.skyfeed;
          Environment = [ "FP6_NMEA_SOCKET=${cfg.satelliteFeed.nmeaSocket}" ];
          StandardInput = "socket";
          StandardOutput = "socket";
          StandardError = "journal";
          DynamicUser = true;
          SupplementaryGroups = [ "geoclue" ];
          RuntimeMaxSec = "4h";
          # sandbox
          CapabilityBoundingSet = "";
          NoNewPrivileges = true;
          PrivateNetwork = true;     # Unix path sockets still work
          PrivateDevices = true;
          PrivateTmp = true;
          ProtectSystem = "strict";
          ProtectHome = true;
          ProtectKernelTunables = true;
          ProtectKernelModules = true;
          ProtectKernelLogs = true;
          ProtectControlGroups = true;
          ProtectClock = true;
          ProtectHostname = true;
          ProtectProc = "invisible";
          ProcSubset = "pid";
          RestrictAddressFamilies = [ "AF_UNIX" ];
          RestrictNamespaces = true;
          RestrictRealtime = true;
          LockPersonality = true;
          MemoryDenyWriteExecute = true;
          SystemCallArchitectures = "native";
          SystemCallFilter = [ "@system-service" "~@privileged" "~@resources" ];
          UMask = "0077";
        };
      };
    })

    (lib.mkIf (cfg.apparmor != "disable") {
      security.apparmor.policies.fp6-sensor-apps = {
        state = cfg.apparmor;
        path = ./fp6-sensor-apps.apparmor;
      };
      system.checks = [
        (pkgs.runCommand "fp6-sensor-apps-apparmor-compile" { } ''
          ${pkgs.apparmor-parser}/bin/apparmor_parser --config-file /dev/null -Q -K \
            -I ${config.environment.etc."apparmor.d".source} \
            ${lib.concatMapStringsSep " " (p: "-I ${p}/etc/apparmor.d") config.security.apparmor.packages} \
            ${./fp6-sensor-apps.apparmor} 2>&1 | { grep -v 'Cache read/write disabled' || true; }
          test "''${PIPESTATUS[0]}" = 0
          touch $out
        '')
      ];
    })

    (lib.mkIf cfg.barometerLogger {
      systemd.user.services.fp6-baro-log = {
        description = "Pressure sample for the Altimeter (opt-in)";
        unitConfig.ConditionPathExists = "%h/.config/fp6-sensor-apps/baro-log-enabled";
        serviceConfig = {
          Type = "oneshot";
          ExecStart = "${pkg}/bin/fp6-sensor-cli baro-log";
          Nice = 10;
        };
      };
      systemd.user.timers.fp6-baro-log = {
        description = "Pressure log every 15 minutes (runs only if enabled in the Altimeter)";
        wantedBy = [ "timers.target" ];
        # Calendar-based, so the timer keeps firing even while the
        # service is skipped by its condition (logger off).
        timerConfig = {
          OnCalendar = "*:0/15";
          AccuracySec = "1min";
        };
      };
    })
  ]);
}
