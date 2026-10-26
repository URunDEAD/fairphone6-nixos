{ config, lib, ... }:
let cfg = config.hardware.fp6IioSensors;
in {
  options.hardware.fp6IioSensors.enable = lib.mkEnableOption "FP6 APPS-owned IIO sensors before Lomiri startup";
  config = lib.mkIf cfg.enable {
    hardware.sensor.iio.enable = true;
    # Preserve the previous FP6 installation's exact anti-SSC rule. There is
    # no SSC registry service on this image; the physical sensors are on APPS
    # buses. Letting libssc discover fastrpc caused long probes and claim-time
    # crashes in the original 3.9 installation. Nixpkgs also enables SSC in 3.9.
    services.udev.extraRules = lib.mkAfter ''
      SUBSYSTEM=="misc", KERNEL=="fastrpc-*", ENV{IIO_SENSOR_PROXY_TYPE}=""
    '';
    # Qt5's iio plugin registers its sensor backend only when the bus name
    # already exists. Order the Type=dbus proxy before display-manager startup.
    systemd.services.iio-sensor-proxy = {
      wantedBy = [ "multi-user.target" ];
      before = [ "display-manager.service" ];
      startLimitIntervalSec = 60;
      startLimitBurst = 3;
      serviceConfig = { Restart = "on-failure"; RestartSec = 5; };
    };
    systemd.services.display-manager = {
      wants = [ "iio-sensor-proxy.service" ];
      after = [ "iio-sensor-proxy.service" ];
    };
  };
}
