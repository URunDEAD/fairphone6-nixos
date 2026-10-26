# Do Not Disturb, bedtime mode and notification history for Lomiri on the FP6.
# Notes and live results: results/feature-push-20261024/notifications/README.md.
#
#  * fp6-notifyd (Rust, user service): the effective DND/bedtime state from
#    the manual switch, the schedules and bedtime (GSettings
#    com.fairphone6.notifications, written as dnd-active / bedtime-active /
#    wallpaper-dim); com.fairphone6.Dnd.ShouldAlert for the telephony daemons
#    (calls/SMS from everyone / contacts / favourites / nobody, repeated
#    callers within 15 min, allowed numbers); the status-bar indicator
#    com.lomiri.indicator.dnd ("Do Not Disturb" and "Bedtime mode" switches,
#    moon icon while on); bedtime greyscale through the display-comfort key;
#    the notification history (bus monitor on Notify; off by default, text
#    off by default, retention 7 days, 0600 in ~/.local/share/fp6-notifyd).
#  * Lomiri (lomiri-dnd.patch, QML, overlay lib.mkOrder 2900):
#    Notifications.qml decides per bubble (no sound, no wake, no bubble when
#    silenced; alarms / snap decisions / critical / OSD / allowed apps pass),
#    Wallpaper.qml gets the bedtime dim rectangle.
#  * lomiri-notifications (lomiri-notifications-app-name.patch): the server
#    copies Notify's app_name into the hint x-fp6-app-name (for "apps allowed").
#  * lomiri-telephony-service (telephony-service-dnd.patch, coordinator-approved
#    hook; cellular/telephony-service-fp6.nix overrides this package, so the
#    fp6-daemons build inherits it): approver and SMS indicator ask
#    ShouldAlert (400 ms, falls back to ringing) and skip ringtone/vibration
#    and mark the bubble x-fp6-dnd=silenced|allowed. Cell broadcasts untouched.
#  * System Settings > Do Not Disturb (settings-page/, QML).
#
# Defaults: DND off, schedules present but off, calls and messages from
# favourites + repeated callers, bubbles hidden and no wake while silenced,
# bedtime off (22:30-07:00, DND + greyscale + 60 % wallpaper dim when used),
# history off.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.notifications;
  package = pkgs.callPackage ./package.nix { inherit (cfg) greyscaleKey; };
  schema = pkgs.callPackage ./schema.nix { };
  lssPlugin = pkgs.callPackage ./lomiri-system-settings-dnd.nix { };
  user = config.services.fp6LomiriSensors.user or "user";
  indicatorFile = pkgs.runCommand "fp6-dnd-indicator" { } ''
    mkdir -p $out/share/ayatana/indicators
    cat > $out/share/ayatana/indicators/com.lomiri.indicator.dnd <<EOF
    [Indicator Service]
    Name=fp6-notifyd
    ObjectPath=/com/lomiri/indicator/dnd
    Position=45

    [phone]
    ObjectPath=/com/lomiri/indicator/dnd/phone
    Position=45

    [desktop]
    ObjectPath=/com/lomiri/indicator/dnd/desktop
    EOF
  '';
in {
  options.hardware.fairphone6.notifications = {
    enable = lib.mkEnableOption ''
      Do Not Disturb (status-bar switch, schedules, exceptions), bedtime mode
      and the (default off) notification history for Lomiri'';
    greyscaleKey = lib.mkOption {
      type = lib.types.str;
      default = "/com/fairphone6/display-comfort/greyscale-active";
      description = ''
        dconf key of the display-comfort greyscale switch that bedtime turns
        on (and restores afterwards). Empty: bedtime never touches colours.
      '';
    };
    telephony = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Patch lomiri-telephony-service so calls/SMS follow DND and its exceptions.";
    };
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = lib.mkOrder 2900 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-dnd.patch ];
          # Notifications.qml / Wallpaper.qml read com.fairphone6.notifications.
          buildInputs = (old.buildInputs or [ ]) ++ [ schema ];
          passthru = (old.passthru or { }) // { fp6Dnd = true; };
        });
        lomiri-notifications = lprev.lomiri-notifications.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-notifications-app-name.patch ];
        });
        lomiri-system-settings = lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ lssPlugin ];
        });
      } // lib.optionalAttrs cfg.telephony {
        lomiri-telephony-service = lprev.lomiri-telephony-service.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./telephony-service-dnd.patch ];
          passthru = (old.passthru or { }) // { fp6Dnd = true; };
        });
      });
    }) ];

    environment.systemPackages = [ package indicatorFile ];
    environment.pathsToLink = [ "/share/ayatana" ];

    systemd.user.services.fp6-notifyd = {
      description = "FP6 Do Not Disturb, bedtime mode and notification history";
      wantedBy = [ "graphical-session.target" "lomiri-indicators.target" "ayatana-indicators.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      # Not for the LightDM greeter's user manager.
      unitConfig.ConditionUser = user;
      serviceConfig = {
        ExecStart = lib.getExe package;
        Restart = "on-failure";
        RestartSec = 5;
      };
    };

    system.build.fp6-notifications = {
      inherit package schema lssPlugin indicatorFile;
      lomiri = pkgs.lomiri.lomiri;
      lomiri-notifications = pkgs.lomiri.lomiri-notifications;
      telephony = pkgs.lomiri.lomiri-telephony-service;
    };

    assertions = [{
      assertion = config.services.fp6LomiriSensors.enable or false;
      message = "fp6 notifications run in the Lomiri session of services.fp6LomiriSensors.user.";
    }];
  };
}
