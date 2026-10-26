# FP6 assisted GPS (A-GPS) for the modem GNSS engine: XTRA predicted orbits,
# UTC time and a coarse position, injected over raw QMI LOC (fp6-agps.py).
# Results and TTFF numbers: results/feature-push-20261024/cellular/agps-README.md
#
# What runs when (the user's location toggle = fp6-gnss-nmea.service):
#  * fp6-agps-xtra.timer / .service: downloads the XTRA file (~65 KB, no
#    user data; the server sees the IP address) into /var/cache/fp6-agps when
#    it is older than refreshHours. It runs only over the allowed networks
#    (default wifi + mobile data). By default it also runs while location is
#    off (xtra.downloadWhenLocationOff). It never injects.
#  * fp6-agps.service: runs only while location is ON (BindsTo the GNSS
#    unit). It injects time (only if NTP-synchronized), XTRA and a coarse
#    position (cached last fix, or BeaconDB if networkLocation is on), then
#    answers the engine's own assistance requests. It is stopped with the
#    toggle.
#  * the last fix is cached by fp6-loc-nmea rounded to 0.05 degrees
#    (~5 km), mode 0600, owner geoclue, in /var/lib/fp6-agps.
#  * networkLocation (default OFF): WiFi BSSIDs and signal strengths
#    (not SSIDs; "_nomap" and hidden networks skipped) are sent to BeaconDB
#    when location is turned on, and GeoClue's WiFi source uses BeaconDB too.
#  * No SUPL: XTRA covers the TTFF case; the public SUPL servers are
#    Google's (supl.google.com), which would get the serving cell ID.
# Modem-side: injected data lives in the GNSS engine (RAM EFS with rmtfs -r);
# no NV item, engine lock or operation mode is changed here.
{ config, lib, pkgs, ... }:
let
  cfg = config.services.fp6Agps;
  gnss = config.services.fp6Cellular.gnss;
  stateDir = "/var/lib/fp6-agps";
  cacheDir = "/var/cache/fp6-agps";
  fp6-agps = pkgs.writers.writePython3Bin "fp6-agps" {
    flakeIgnore = [ "E501" "E731" "E741" "W503" ];
  } (builtins.readFile ./fp6-agps.py);
  configFile = pkgs.writeText "fp6-agps.json" (builtins.toJSON {
    inherit cacheDir stateDir;
    gnssUnit = "fp6-gnss-nmea.service";
    xtra = {
      inherit (cfg.xtra) enable download servers networks refreshHours downloadWhenLocationOff;
      preferModemServers = cfg.xtra.preferModemServers;
      maxAgeDays = 7;
    };
    time = { inherit (cfg.time) enable; };
    position = {
      inherit (cfg) lastFix;
      network = cfg.networkLocation.enable;
      networkUrl = cfg.networkLocation.url;
    };
  });
  agpsPath = [ pkgs.systemd ] ++ lib.optional cfg.networkLocation.enable config.networking.networkmanager.package;
  hardening = {
    User = "geoclue";
    Group = "geoclue";
    NoNewPrivileges = true;
    ProtectSystem = "strict";
    ProtectHome = true;
    PrivateTmp = true;
    PrivateDevices = true;
    ProtectKernelTunables = true;
    ProtectKernelModules = true;
    ProtectControlGroups = true;
    RestrictNamespaces = true;
    LockPersonality = true;
    MemoryDenyWriteExecute = true;
    SystemCallArchitectures = "native";
    Environment = [ "SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt" ];
  };
in {
  options.services.fp6Agps = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = config.services.fp6Cellular.enable && gnss.enable;
      defaultText = lib.literalExpression "fp6Cellular.enable && fp6Cellular.gnss.enable";
      description = "Assisted GPS (XTRA, time, coarse position) for the FP6 modem GNSS.";
    };
    xtra = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Inject XTRA predicted orbits (while location is on).";
      };
      download = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Periodically download the XTRA file (contains no user data).";
      };
      downloadWhenLocationOff = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Keep the XTRA cache fresh while location is off too (no injection then).";
      };
      networks = lib.mkOption {
        type = lib.types.listOf (lib.types.enum [ "wifi" "mobile" "other" ]);
        default = [ "wifi" "mobile" ];
        description = "Default-route types the XTRA download may use (\"other\" = e.g. USB networking).";
      };
      refreshHours = lib.mkOption {
        type = lib.types.ints.positive;
        default = 24;
        description = "Re-download when the cached file is older than this (an XTRA file covers 7 days).";
      };
      preferModemServers = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Use the XTRA URLs the modem reports (Get Predicted Orbits Data Source) first.";
      };
      servers = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [
          "https://path1.xtracloud.net/xtra3Mgrbej.bin"
          "https://path2.xtracloud.net/xtra3Mgrbej.bin"
          "https://path3.xtracloud.net/xtra3Mgrbej.bin"
        ];
        description = "Fallback XTRA URLs (what this modem firmware names itself).";
      };
      interval = lib.mkOption {
        type = lib.types.str;
        default = "3h";
        description = "How often the timer checks whether the cache needs a refresh.";
      };
    };
    time.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Inject UTC time from the system clock (only while NTP-synchronized).";
    };
    lastFix = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Cache the last GNSS fix on disk, rounded to 0.05 degrees (~5 km), and
        inject it as the coarse position. Local only. Turning this off deletes the cache.
      '';
    };
    networkLocation = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = ''
          Opt-in network location through BeaconDB: while location is on,
          nearby WiFi BSSIDs and signal strengths are sent to `url` for a
          coarse position to inject, and GeoClue's WiFi source uses it too.
        '';
      };
      url = lib.mkOption {
        type = lib.types.str;
        default = "https://api.beacondb.net/v1/geolocate";
        description = "Ichnaea-compatible geolocate endpoint.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = config.services.fp6Cellular.enable && gnss.enable;
      message = "fp6Agps needs services.fp6Cellular.gnss (the QMI LOC GNSS source).";
    }];
    environment.systemPackages = [ fp6-agps ];
    environment.etc."fp6-agps.json".source = configFile;
    system.build.fp6-agps = fp6-agps;

    # fp6-loc-nmea stores the rounded last fix here (shared state dir).
    services.fp6Cellular.gnss.fixCache = lib.mkIf cfg.lastFix "${stateDir}/last-fix.json";
    systemd.services.fp6-gnss-nmea.serviceConfig.StateDirectory = "fp6-agps";
    systemd.services.fp6-gnss-nmea.serviceConfig.StateDirectoryMode = "0700";
    systemd.tmpfiles.rules = lib.optional (!cfg.lastFix) "r ${stateDir}/last-fix.json";

    services.geoclue2 = lib.mkIf cfg.networkLocation.enable {
      enableWifi = true;
      geoProviderUrl = cfg.networkLocation.url;
    };

    # Location ON only: started with the toggle, stopped with it.
    systemd.services.fp6-agps = {
      description = "FP6 assisted GPS: inject XTRA/time/coarse position while location is on";
      wantedBy = [ "fp6-gnss-nmea.service" ];
      bindsTo = [ "fp6-gnss-nmea.service" ];
      after = [ "fp6-gnss-nmea.service" "fp6-loc-engine-lock.service" ];
      wants = lib.optional (cfg.xtra.enable && cfg.xtra.download) "fp6-agps-xtra.service";
      path = agpsPath;
      serviceConfig = hardening // {
        ExecStart = "${lib.getExe fp6-agps} --config /etc/fp6-agps.json serve";
        StateDirectory = "fp6-agps";
        StateDirectoryMode = "0700";
        CacheDirectory = "fp6-agps";
        CacheDirectoryMode = "0750";
        Restart = "on-failure";
        RestartSec = 10;
        RestrictAddressFamilies = [ "AF_UNIX" "AF_QIPCRTR" ]
          ++ lib.optionals cfg.networkLocation.enable [ "AF_INET" "AF_INET6" ];
      };
    };

    systemd.services.fp6-agps-xtra = lib.mkIf (cfg.xtra.enable && cfg.xtra.download) {
      description = "FP6 assisted GPS: refresh the cached XTRA predicted-orbits file";
      after = [ "network-online.target" ];
      path = [ pkgs.systemd ];
      serviceConfig = hardening // {
        Type = "oneshot";
        ExecStart = "${lib.getExe fp6-agps} --config /etc/fp6-agps.json xtra-download";
        CacheDirectory = "fp6-agps";
        CacheDirectoryMode = "0750";
        RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" ];
        TimeoutStartSec = 180;
      };
    };
    systemd.timers.fp6-agps-xtra = lib.mkIf (cfg.xtra.enable && cfg.xtra.download) {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "3min";
        OnUnitActiveSec = cfg.xtra.interval;
        RandomizedDelaySec = "10min";
      };
    };
  };
}
