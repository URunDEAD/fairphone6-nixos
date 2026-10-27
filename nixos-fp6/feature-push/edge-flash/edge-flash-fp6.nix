# Edge lighting (feature push 2026-10-24, roadmap P2): the FP6 has no
# notification LED. A notification that arrives while the display is off
# lights the screen edges for a moment (Samsung "edge lighting" style) in
# the app's or a fixed colour, instead of turning the display on.
# Notes, measurements/estimates, previews and the always-on display research:
# results/feature-push-20261024/edge-flash/README.md.
#
#  * fp6-lomiri-powerd (sensors module): org.gtk.Actions on
#    /com/fairphone6/EdgeFlash (system bus, com.lomiri.Repowerd): "flash"
#    ("MS:LEVEL") raises the panel brightness to a low level while the
#    display stays logically off (no DisplayPowerStateChange, touch stays in
#    LP, idle/lock untouched); refused with the display on, in a call, in a
#    pocket or with the proximity sensor near. "stop", state "flashing".
#  * Lomiri (lomiri-edge-flash.patch + qml/Fp6EdgeFlash.qml, overlay
#    lib.mkOrder 3600, applied in postPatch after every other module's
#    Lomiri edits): Notification.qml offers each bubble that DND lets through
#    to Fp6EdgeFlash (Shell.qml, above everything), which draws an opaque
#    black frame with a rounded-edge glow (one shader pass, stepped at
#    ~30 Hz) and asks powerd for the backlight. Calls, alarms, OSDs and
#    critical notifications wake the display as before. Optional reminder
#    while unseen. Battery saver: off unless "also with battery saver".
#  * System Settings > Edge Lighting (settings-page/): on/off (default off),
#    only calls/messages, app colour or a fixed colour, brightness, reminder,
#    battery saver, Preview. GSettings com.fairphone6.edge-flash (b/i/s/as).
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.edgeFlash;
  pkg = pkgs.callPackage ./package.nix { };
  cornerRadius = config.hardware.fairphone6.quickSettings.cornerRadiusPx or 110;
  withBattery = config.hardware.fairphone6.battery.enable or false;
in {
  options.hardware.fairphone6.edgeFlash.enable = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = ''
      Install edge lighting (Lomiri patch, Settings page). The glow itself
      stays off until the user turns it on in Settings > Edge Lighting.
    '';
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      { assertion = (config.services.fp6LomiriSensors.enable or false) && config.services.fp6LomiriSensors.screenPower;
        message = "edgeFlash needs fp6-lomiri-powerd with screen power (services.fp6LomiriSensors.enable + screenPower) for the EdgeFlash actions."; }
      { assertion = config.hardware.fairphone6.notifications.enable or false;
        message = "edgeFlash's Notification(s).qml patch context includes the notifications module's DND code (notifications.enable)."; }
      { assertion = config.hardware.fairphone6.quickSettings.enable or false;
        message = "edgeFlash's Shell.qml patch context includes the quick-settings shade (quickSettings.enable)."; }
      { assertion = config.hardware.fairphone6.privacyLock.enable or false;
        message = "edgeFlash's Shell.qml patch context includes privacy-lock's display-off input block (privacyLock.enable)."; }
    ];

    # After every other Lomiri overlay (highest so far: mir-upgrade 3500).
    nixpkgs.overlays = lib.mkOrder 3600 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          # Fp6EdgeFlash.qml reads com.fairphone6.edge-flash (+ battery saver).
          buildInputs = (old.buildInputs or [ ]) ++ [ pkg.schema ] ++ lib.optional withBattery pkg.batterySchema;
          postPatch = (old.postPatch or "") + ''
            # FP6 edge lighting (after every other module's Lomiri edits)
            patch -p1 --no-backup-if-mismatch < ${./lomiri-edge-flash.patch}
            install -m644 ${./qml/Fp6EdgeFlash.qml} qml/Notifications/Fp6EdgeFlash.qml
            substituteInPlace qml/Notifications/Fp6EdgeFlash.qml \
              --replace-fail '@fp6EdgeCornerRadiusPx@' '${toString cornerRadius}'
            grep -q 'fp6EdgeFlash.offer' qml/Notifications/Notification.qml
          '';
          passthru = (old.passthru or { }) // { fp6EdgeFlash = true; };
        });
        lomiri-system-settings = lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ pkg.lssPlugin ];
        });
      });
    }) ];

    environment.systemPackages = [ pkg.schema ];

    system.build.fp6-edge-flash = {
      inherit (pkg) schema lssPlugin;
      lomiri = pkgs.lomiri.lomiri;
    };
  };
}
