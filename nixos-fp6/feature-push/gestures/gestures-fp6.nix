# Gesture shortcuts (feature push 2026-10-24): shake to toggle the flashlight /
# open an app / run a command, and power key double/triple press shortcuts.
# Notes and test results: results/feature-push-20261024/gestures/README.md.
#
#  * fp6-gestures (Rust, user service of the Lomiri session): GSettings
#    com.fairphone6.gestures, read live. Shake: polls the ICM-42631
#    accelerometer through IIO sysfs (in_accel_*_raw, the same attributes
#    iio-sensor-proxy polls; sampling_frequency is never written) only while
#    the shake is on: idle 15 Hz with the display on, 10 Hz off (only if the
#    action may run locked; paused otherwise), 50 Hz for 2 s once the phone
#    moves (> 0.8 g). The IMU is on a bit-banged spi-gpio bus: each axis read
#    is ~0.1 ms of kernel CPU (50 Hz = 4.7 % of a core, 25 Hz = 2.5 %).
#  * Power key: fp6-lomiri-powerd counts presses (com.fairphone6.PowerKey on
#    com.lomiri.Repowerd). fp6-gestures sets the max press count from the
#    settings (0 = nothing assigned = the instant single press of before).
#  * Actions: the indicator-power flashlight action (status bar stays in
#    sync), url-dispatcher for apps, /bin/sh as the user for commands
#    (timeout), hfd vibration as feedback.
#  * Pocket mode, flip to silence, raise to wake, back tap (follow-up,
#    2026-10-26): flip/raise/back tap sleep on the ICM-42631's wake-on-motion
#    interrupt (IIO event, no polling at rest); pocket = iio-sensor-proxy
#    proximity near + dark (direct lux read) > 1 s, gated in
#    fp6-lomiri-powerd (com.fairphone6.Gestures SetPocket / Peek).
#  * Back tap tuning (2026-10-28): sensitivity slider, guided calibration
#    and a live test mode on the Settings page (GSettings back-tap-mode /
#    -profile / -log, no extra service); touch, typing, table and motion
#    rejection; 10 Hz instead of 50 Hz while walking. Evaluation:
#    `cargo test eval_report -- --nocapture`, real traces: `eval-taps`.
#  * Stay on while held (2026-10-27, edge-flash README): powerd asks
#    (HoldCheck) shortly before the idle dim; one 1.5 s IMU burst decides
#    "held in a reading pose" (hold.rs); nothing is read while it is off.
#  * Volume keys long press = next/previous track with the display off
#    (read-only evdev, MPRIS), volume put back afterwards.
#  * System Settings > Gesture Shortcuts (QML plugin page).
#
# Everything is off by default: the module installs the service and the page;
# the user turns shortcuts on in Settings.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.gestures;
  package = pkgs.callPackage ./package.nix { };
  lssPlugin = pkgs.callPackage ./lomiri-system-settings-gestures.nix { fp6-gestures = package; };
  user = config.services.fp6LomiriSensors.user or "user";
in {
  options.hardware.fairphone6.gestures.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = ''
      Install fp6-gestures and the Settings > Gesture Shortcuts page. The
      shortcuts themselves stay off until the user enables them there.
    '';
  };

  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = config.services.fp6LomiriSensors.enable or false;
      message = "fp6 gestures need fp6-lomiri-powerd (services.fp6LomiriSensors) for the power key and display state.";
    }];

    environment.systemPackages = [ package ];

    # Wake-on-motion for fp6-gestures (user "${user}", group input): read the
    # accelerometer's IIO chardev (only to get the event fd; it is closed
    # again, the buffer stays free) and switch the WoM event on/off. Raw
    # reads by iio-sensor-proxy and the sensor apps are unaffected.
    services.udev.packages = [ (pkgs.writeTextDir "lib/udev/rules.d/82-fp6-gestures-wom.rules" ''
      SUBSYSTEM=="iio", KERNEL=="iio:device*", ATTR{name}=="icm426*-accel", GROUP="input", MODE="0640", RUN+="${pkgs.coreutils}/bin/chgrp input /sys%p/events/in_accel_x|y|z_roc_rising_en /sys%p/events/in_accel_x|y|z_roc_rising_value", RUN+="${pkgs.coreutils}/bin/chmod 0664 /sys%p/events/in_accel_x|y|z_roc_rising_en /sys%p/events/in_accel_x|y|z_roc_rising_value"
    '') ];
    # The phone user is in group input (also needed for the volume keys and
    # the touchscreen veto, read-only evdev).
    system.build.fp6-gestures = { inherit package lssPlugin; };

    nixpkgs.overlays = [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri-system-settings = lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ lssPlugin ];
        });
      });
    }) ];

    systemd.user.services.fp6-gestures = {
      description = "FP6 gesture shortcuts (shake, power key, pocket, flip, raise, back tap, volume keys)";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      # Not for the LightDM greeter's user manager.
      unitConfig.ConditionUser = user;
      serviceConfig = {
        ExecStart = "${lib.getExe package} run";
        Restart = "on-failure";
        RestartSec = 5;
        # Custom commands run as children of this unit: they stop with it.
        KillMode = "control-group";
        Nice = 5;
      };
    };
  };
}
