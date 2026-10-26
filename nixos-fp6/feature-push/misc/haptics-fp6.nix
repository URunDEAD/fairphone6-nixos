{ config, lib, pkgs, ... }:
# FP6 vibration strength (aw86938 LRA, driver aw86927-haptics, /dev/input/eventN).
#
# The hardware path works: the driver is bound, hfd-service picks the
# FF_RUMBLE input device, and the chip goes to RAM-play (GLBRD5 = 7) with
# no fault bits. Two userspace problems made it feel weak or absent:
#
#  1. hfd 0.2.4 hard-codes FF_RUMBLE strong_magnitude = 0x6000. The driver
#     maps that to PLAYCFG2 gain = magnitude * 0x80 / 0xffff = 0x30 (37.5 %
#     of the driver's 0x80 maximum). The accelerometer showed the case
#     vibration scales linearly with gain: 0x30 -> 452, 0x80 -> 1036
#     (high-pass RMS counts, 300 ms pulses). The patch makes the magnitude
#     configurable (HFD_FF_MAGNITUDE). We use 0xffff, which is gain 0x80, the
#     ceiling the upstream driver itself allows. The boost voltage (7.0 V
#     for aw86938) and the waveform stay as the kernel sets them.
#
#  2. Lomiri key feedback asks for 10 ms. The driver's play work does
#     STOP + standby wait + setup before GO, so a 10 ms request produced no
#     measurable motion at all (accel = baseline). hfd adds the DeviceInfo
#     property VibrateDurationExtraMs to every request. However, hfd runs
#     without DEVICEINFO_DEVICE_PATH, so it never found
#     /etc/deviceinfo/devices/fairphone-fp6.yaml ("Unknown prop", Generic
#     device). We set the path for the service and add the property.
#     +15 ms: keypress 10 -> 25 ms, a short but noticeable tick.
#
# Only the hfd-service daemon binary is swapped (ExecStart). pkgs.lomiri is
# left alone, so lomiri, the QtFeedback plugin and the other reverse
# dependencies of hfd-service are not rebuilt.
let
  cfg = config.hardware.fairphone6.haptics;
  hfdPatched = pkgs.lomiri.hfd-service.overrideAttrs (old: {
    pname = "hfd-service-fp6";
    patches = (old.patches or []) ++ [ ./hfd-ff-magnitude.patch ];
  });
in {
  options.hardware.fairphone6.haptics = {
    magnitude = lib.mkOption {
      type = lib.types.ints.between 1 65535;
      default = 65535;
      description = "FF_RUMBLE strong_magnitude hfd uploads (aw86927 gain = value*0x80/0xffff).";
    };
    vibrateDurationExtraMs = lib.mkOption {
      type = lib.types.ints.between 0 100;
      default = 15;
      description = "DeviceInfo VibrateDurationExtraMs: added by hfd to every vibration request.";
    };
  };

  config = {
    systemd.services.hfd-service.serviceConfig.ExecStart = [
      "" "${hfdPatched}/libexec/hfd-service"
    ];
    systemd.services.hfd-service.environment = {
      HFD_FF_MAGNITUDE = toString cfg.magnitude;
      DEVICEINFO_DEVICE_PATH = "/etc/deviceinfo/devices";
    };

    # environment.etc.<n>.text is types.lines, so this appends under the
    # "fairphone-fp6:" key defined in nixos-fp6/configuration.nix.
    environment.etc."deviceinfo/devices/fairphone-fp6.yaml".text = lib.mkAfter
      "  VibrateDurationExtraMs: ${toString cfg.vibrateDurationExtraMs}\n";
  };
}
