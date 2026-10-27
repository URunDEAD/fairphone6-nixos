# FP6 quick settings (2026-10-27): an Android/iOS-like control centre in the
# pulled-down Lomiri panel, and rounded-corner safe areas for the status bar.
# results/feature-push-20261024/quick-settings/README.md
#
#  * Control centre (lomiri-quick-settings.patch + qml/ -> qml/Panel/QuickSettings):
#    pulling the status bar down opens date/time, a grid of round tiles (2 rows,
#    expand for more; small or wide tiles), brightness/volume sliders, media
#    controls and the notifications (grouped by app, swipe to dismiss, Clear
#    all). Tap toggles; hold (or the arrow of a wide tile) opens the
#    indicator's page or the Settings page. Indicator pages stay reachable by
#    tapping their icon in the expanded row; "Quick settings" goes back.
#    Backends: the indicators' GActions over D-Bus (network, Bluetooth,
#    location, display, power, sound, DND, screen recording, battery) and the
#    FP6 GSettings keys (display-comfort night light/greyscale). No polling.
#  * Edit mode (pencil button; not on the lock screen) and Settings > Quick
#    settings: drag to reorder/add/remove, small/wide, tiles per row (4-6),
#    rows before expanding (1-3), slider order/visibility, reset. Stored in
#    com.fairphone6.quick-settings ('as'/'i'/'s'/'b' keys only).
#  * Lock screen: tiles marked "unlock" (Wi-Fi, Bluetooth, mobile data,
#    airplane, location, hotspot, screen recording, scan QR) ask for the
#    passcode instead of switching; the indicator pages in
#    lockedRestrictedIndicators show an "Unlock" card; notification text
#    follows Lomiri's "hide content while locked".
#  * Dark theme tile: fp6-quick-settings-theme (user service) keeps the key and
#    the UITK theme.ini / GTK settings in sync both ways.
#  * Rounded corners (lomiri-corner-insets.patch): the status-bar rows (collapsed
#    and expanded, indicators and app menus) and the title/recording dot keep
#    clear of the display's rounded corners: inset = r - sqrt(r^2 - (r - top)^2)
#    for content starting `top` below the edge, with r = cornerRadiusPx.
#    Composes with lomiri-polish's camera-hole gap (the overflow maths are
#    measured from the inset bar edge).
#
# Overlay at mkOrder 3100: after every other module's Lomiri overlay; both
# patches are applied in postPatch, i.e. after lomiri-polish's substitutions.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.quickSettings;
  pkg = pkgs.callPackage ./package.nix { };
  user = config.services.fp6LomiriSensors.user or "user";
  restricted = "[" + lib.concatMapStringsSep ", " (s: ''"${s}"'') cfg.lockedRestrictedIndicators + "]";
in {
  options.hardware.fairphone6.quickSettings = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Android/iOS-like control centre in the Lomiri panel, and rounded-corner insets for the status bar.";
    };
    cornerRadiusPx = lib.mkOption {
      type = lib.types.ints.between 0 300;
      default = 110;
      description = ''
        Radius of the display's rounded corners in physical pixels (0: no
        insets). Not published for the FP6; estimated (see the README).
        Settings > Quick settings > "Show the rounded-corner safe area" draws
        the arcs to check it on the phone.
      '';
    };
    lockedRestrictedIndicators = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "indicator-network" "ayatana-indicator-bluetooth" "fp6-location-indicator"
                  "fp6-privacy-indicator" "fp6-usb-drive-indicator" "fp6-screenrecord-indicator" ];
      description = "Indicator pages (identifiers) that ask for the passcode on the lock screen.";
    };
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = lib.mkOrder 3100 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          # Panel.qml reads com.fairphone6.quick-settings (wrapGAppsHook adds the schema).
          buildInputs = (old.buildInputs or [ ]) ++ [ pkg.schema ];
          postPatch = (old.postPatch or "") + ''
            # FP6 quick settings + rounded-corner insets (after lomiri-polish's edits)
            patch -p1 --no-backup-if-mismatch < ${./lomiri-corner-insets.patch}
            patch -p1 --no-backup-if-mismatch < ${./lomiri-quick-settings.patch}
            mkdir -p qml/Panel/QuickSettings
            cp ${./qml}/* qml/Panel/QuickSettings/
            chmod u+w qml/Panel/QuickSettings/*
            substituteInPlace qml/Panel/Panel.qml \
              --replace-fail '@fp6CornerRadiusPx@' '${toString cfg.cornerRadiusPx}' \
              --replace-fail '@fp6LockedRestricted@' '${restricted}'
            if grep -rn '@fp6' qml/Panel; then echo 'quick-settings: unsubstituted placeholder' >&2; exit 1; fi
          '';
          passthru = (old.passthru or { }) // { fp6QuickSettings = true; };
        });
        lomiri-system-settings = lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ pkg.lssPlugin ];
        });
      });
    }) ];

    environment.systemPackages = [ pkg.schema ];

    systemd.user.services.fp6-quick-settings-theme = {
      description = "FP6 quick settings: dark theme tile <-> app theme";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      unitConfig.ConditionUser = user;
      serviceConfig = {
        ExecStart = lib.getExe pkg.themeHelper;
        Restart = "on-failure";
        RestartSec = 5;
        Nice = 10;
      };
    };

    system.build.fp6-quick-settings = {
      inherit (pkg) schema lssPlugin themeHelper;
      lomiri = pkgs.lomiri.lomiri;
    };

    assertions = [
      { assertion = config.hardware.fairphone6.lomiriPolish.enable or false
          && config.hardware.fairphone6.lomiriPolish.cameraCutout or false;
        message = "quickSettings patches the panel on top of lomiri-polish's camera-cutout layout (lomiriPolish.enable + cameraCutout)."; }
      { assertion = config.hardware.fairphone6.screenCapture.enable or false;
        message = "quickSettings' Panel/Shell patch context includes the screen-capture module's recording dot (screenCapture.enable)."; }
      { assertion = config.hardware.fairphone6.notifications.enable or false;
        message = "quickSettings' Notifications.qml patch context includes the notifications module's DND code (notifications.enable)."; }
    ];
  };
}
