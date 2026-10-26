# Module evaluation check (no build of the full system):
#   nix-instantiate --eval --strict --json nixos-fp6/feature-push/cellular/eval.nix
let
  pin = /nix/store/82jdygd6crh2575x13mdqjmqds5ffwfs-source;
  evaluate = extra: (import (pin + "/nixos/lib/eval-config.nix") {
    system = "aarch64-linux";
    modules = [ ./cellular-fp6.nix ({ pkgs, ... }: {
      system.stateVersion = "26.11";
      services.desktopManager.lomiri.enable = true;
      users.users.user = { isNormalUser = true; uid = 10000; };
      # Mirrors configuration.nix's rmtfs unit (read-only EFS shadow).
      systemd.services.rmtfs.serviceConfig.ExecStart = "${pkgs.rmtfs}/bin/rmtfs -r -P -s";
    }) extra ];
  }).config;
  on = evaluate { };
  rw = evaluate ({ lib, ... }: { systemd.services.rmtfs.serviceConfig.ExecStart = lib.mkForce "rmtfs -P -s"; });
  failed = c: map (a: a.message) (builtins.filter (a: !a.assertion) c.assertions);
in {
  enabledByDefault = on.services.fp6Cellular.enable;
  failedAssertions = failed on;
  modemmanager = on.networking.modemmanager.enable;
  ofono = on.services.ofono.enable;
  ofonoExec = on.systemd.services.ofono.serviceConfig.ExecStart;
  nmPackage = on.networking.networkmanager.package.name;
  premux = { pre = on.systemd.services.ofono.serviceConfig.ExecStartPre; post = on.systemd.services.ofono.serviceConfig.ExecStartPost; };
  uim = on.systemd.services.msm-modem-uim-selection.serviceConfig.ExecStart;
  gnss = on.systemd.services.fp6-gnss-nmea.serviceConfig.ExecStart;
  geoclueConfD = on.environment.etc."geoclue/conf.d/90-fp6-gnss.conf".text;
  geoclueNmea = on.services.geoclue2.enableNmea;
  nuntium = on.systemd.user.services ? nuntium;
  account = on.systemd.user.services.fp6-ofono-account.serviceConfig.ExecStart;
  userGroups = on.users.users.user.extraGroups;
  indicatorNetworkCondition = on.systemd.user.services.lomiri-indicator-network.unitConfig.ConditionUser;
  writableRmtfsRejected = failed rw;
  agps = {
    enabled = on.services.fp6Agps.enable;
    networkLocationDefault = on.services.fp6Agps.networkLocation.enable;
    geoclueWifi = on.services.geoclue2.enableWifi;
    serveBindsTo = on.systemd.services.fp6-agps.bindsTo;
    serveWantedBy = on.systemd.services.fp6-agps.wantedBy;
    serveFamilies = on.systemd.services.fp6-agps.serviceConfig.RestrictAddressFamilies;
    timer = on.systemd.timers.fp6-agps-xtra.timerConfig;
    gnssStateDir = on.systemd.services.fp6-gnss-nmea.serviceConfig.StateDirectory;
  };
  agpsNetworkOptIn = let c = evaluate { services.fp6Agps.networkLocation.enable = true; services.fp6Agps.lastFix = false; }; in {
    geoclueWifi = c.services.geoclue2.enableWifi;
    url = c.services.geoclue2.geoProviderUrl;
    serveFamilies = c.systemd.services.fp6-agps.serviceConfig.RestrictAddressFamilies;
    gnss = c.systemd.services.fp6-gnss-nmea.serviceConfig.ExecStart;
    tmpfiles = c.systemd.tmpfiles.rules;
  };
}
