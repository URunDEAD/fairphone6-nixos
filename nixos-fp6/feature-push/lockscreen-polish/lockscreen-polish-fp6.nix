# Lock screen polish and separate wallpapers for Lomiri on the FP6.
# Notes, previews and the phone checklist:
# results/feature-push-20261024/lockscreen/README.md
#
# hardware.fairphone6.lockscreenPolish (lomiri-lockscreen-fingerprint.patch):
#  * The FP6 fingerprint sensor is in the power button on the right edge
#    (LineageOS FP6 overlay: config_is_powerbutton_fps = true,
#    physical_fingerprint_sensor_center_screen_location_y = 1100 px of 2484).
#    The greeter's centre fingerprint icon (fingerprint/
#    lomiri-greeter-fingerprint-feedback.patch) read as an under-display
#    sensor; it is hidden and replaced by Greeter/Fp6FingerprintHint.qml: a
#    soft glow on the right edge at the sensor, a short pulse when the lock
#    screen appears, "Touch the power button to unlock" for ~2.5 s, a flash
#    on an undecided touch, shake + red + double vibration + "Not
#    recognised" on a rejected finger, "Fingerprint sensor busy" when the
#    reader is claimed elsewhere, a flare + ripple on a match, and after the
#    5th failure (Lomiri's own limit, unchanged) "Use your passcode" and a
#    slide to the passcode screen.
#  * UI only: Greeter.qml's identification, failure counting and unlock are
#    unchanged; only the messages and the hooks to the hint differ.
#  * Shown only while biometryd identification runs (greeter active,
#    display on, fingerprint enabled), so nothing animates with the display
#    off; static between events. Quieter during bedtime.
#
# hardware.fairphone6.wallpapers (lomiri-wallpapers.patch + wallpapers/):
#  * Settings > Background & Appearance: lock and home previews, "Set" asks
#    Home screen / Lock screen / Home and lock screens, "Choose from
#    Pictures…", the bundled wallpapers (SYSTEM_SETTINGS_SYSTEM_ART_DIR),
#    home blur and dim, "Use the home wallpaper on the lock screen".
#  * Home = the stock AccountsService BackgroundFile. Lock = a re-encoded copy
#    <greeter data dir>/<user>/fp6-lock-wallpaper-<ms>.<jpg|png> (dir
#    user:lightdm 0770, file 0644), readable by the session's lock screen
#    and the LightDM greeter, like the stock shareWithGreeter copies.
#    No file = the lock screen shows the home wallpaper (as before).
#  * Lomiri: Shell.qml resolves the lock wallpaper for the Greeter; Stage,
#    launcher, spread and notifications keep the home one. Home blur/dim:
#    Wallpaper.qml fp6Blur/fp6Dim, set by Stage.qml only; the bedtime dim
#    (notifications) stays on every wallpaper, lock and home.
#
# Overlay lib.mkOrder 3200: after every other Lomiri patcher of the full
# target (privacy-lock 2800, notifications 2900, quick-settings 3100).
{ config, lib, pkgs, ... }:
let
  lcfg = config.hardware.fairphone6.lockscreenPolish;
  wcfg = config.hardware.fairphone6.wallpapers;
  anyOn = lcfg.enable || wcfg.enable;
  schema = pkgs.callPackage ./schema.nix { inherit (lcfg) sensorCenter sensorLength; };
  defaultWallpaper = "/run/current-system/sw/share/wallpapers/lomiri-default-background.png";
  notificationsOn = config.hardware.fairphone6.notifications.enable or false;
in {
  options.hardware.fairphone6.lockscreenPolish = {
    enable = lib.mkEnableOption ''
      the side-sensor fingerprint hint on the Lomiri lock screen (edge glow
      next to the power button, text cue, failure/success feedback)'';
    sensorCenter = lib.mkOption {
      type = lib.types.float;
      default = 1100.0 / 2484.0;
      description = ''
        Centre of the power-button fingerprint sensor as a fraction of the
        portrait screen height from the top (default: LineageOS FP6 overlay,
        1100 px of 2484). Schema default of com.fairphone6.lockscreen
        fingerprint-sensor-center; can be tuned per user with gsettings.
      '';
    };
    sensorLength = lib.mkOption {
      type = lib.types.float;
      default = 0.075;
      description = ''
        Length of the edge hint as a fraction of the portrait screen height
        (0.075 = 186 px, about the 11 mm power key; estimate).
      '';
    };
  };

  options.hardware.fairphone6.wallpapers = {
    enable = lib.mkEnableOption ''
      separate lock-screen and home-screen wallpapers (System Settings >
      Background & Appearance), home blur/dim, picking from ~/Pictures'';
    greeterDataDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/lightdm-data";
      description = ''
        LightDM greeter data directory (per-user subdirectories, owner the
        user, group lightdm). The lock wallpaper is stored there so the
        LightDM greeter can read it.
      '';
    };
  };

  config = lib.mkIf anyOn {
    assertions = lib.optionals lcfg.enable [{
      assertion = (config.services.fp6BiometrydFprintd.enable or false)
        && (config.services.fp6BiometrydFprintd.greeterFeedback or false);
      message = "lockscreenPolish builds on the greeter fingerprint hooks (services.fp6BiometrydFprintd.greeterFeedback).";
    } {
      assertion = (config.hardware.fairphone6.privacyLock.enable or false)
        && (config.hardware.fairphone6.privacyLock.lomiriShell or false);
      message = "lockscreenPolish places its hint below the privacy-lock emergency sheet (hardware.fairphone6.privacyLock with lomiriShell).";
    }] ++ lib.optionals wcfg.enable [{
      assertion = config.services.xserver.displayManager.lightdm.enable;
      message = "hardware.fairphone6.wallpapers stores the lock wallpaper in LightDM's greeter data directory.";
    }];

    nixpkgs.overlays = lib.mkOrder 3200 [ (final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          patches = (old.patches or [ ])
            ++ lib.optional lcfg.enable ./lomiri-lockscreen-fingerprint.patch
            ++ lib.optional wcfg.enable ./lomiri-wallpapers.patch;
          postPatch = (old.postPatch or "")
            + lib.optionalString lcfg.enable ''
              substituteInPlace qml/Greeter/GreeterView.qml \
                --replace-fail '@FP6_BEDTIME_SCHEMA@' \
                  '${if notificationsOn then "com.fairphone6.notifications" else "com.fairphone6.lockscreen"}'
            ''
            + lib.optionalString wcfg.enable ''
              substituteInPlace qml/Shell.qml \
                --replace-fail '@FP6_GREETER_DATA_DIR@' '${wcfg.greeterDataDir}'
            '';
          # wrapGAppsHook: com.fairphone6.lockscreen in Lomiri's XDG_DATA_DIRS.
          buildInputs = (old.buildInputs or [ ]) ++ [ schema ];
          passthru = (old.passthru or { }) // {
            fp6LockscreenPolish = lcfg.enable;
            fp6Wallpapers = wcfg.enable;
          };
        });
      } // lib.optionalAttrs wcfg.enable {
        lomiri-system-settings = let
          plugin = final.callPackage ./wallpapers/package.nix {
            fp6-lockscreen-schema = schema;
            inherit (wcfg) greeterDataDir;
          };
        in (lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ plugin ];
        })).overrideAttrs (old: {
          nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ final.buildPackages.patch ];
          # Patched copies of the two stock Background QML files (the patch
          # fails the build if upstream changed them).
          postInstall = (old.postInstall or "") + ''
            bg=$out/share/lomiri-system-settings/qml-plugins/background
            for f in MainPage.qml Preview.qml; do
              src=$(readlink -f "$bg/$f")
              cp --remove-destination "$src" "$bg/$f.fp6"
              mv "$bg/$f.fp6" "$bg/$f"
              chmod u+w "$bg/$f"
            done
            patch -d "$bg" -p1 --forward --batch < ${./wallpapers/lss-background-wallpapers.patch}
            substituteInPlace "$bg/MainPage.qml" \
              --replace-fail '@FP6_DEFAULT_WALLPAPER@' '${defaultWallpaper}'
            grep -q fp6Apply "$bg/MainPage.qml"
            grep -q fp6WhereDialog "$bg/Preview.qml"
            test -e "$bg/Fp6WallpaperSection.qml"
            test -e "$bg/Fp6PicturesPage.qml"
          '';
          # The bundled wallpapers in "System art" (stock looks in
          # /usr/share/backgrounds; real files, the plugin skips symlinks).
          preFixup = (old.preFixup or "") + ''
            qtWrapperArgs+=(--set-default SYSTEM_SETTINGS_SYSTEM_ART_DIR "${final.lomiri.lomiri-wallpapers}/share/wallpapers")
          '';
          passthru = (old.passthru or { }) // { fp6WallpapersPlugin = plugin; };
        });
      });
    }) ];

    # gsettings on the command line (e.g. tuning the sensor position).
    environment.systemPackages = [ schema ];

    system.build.fp6-lockscreen-polish = {
      inherit schema;
      lomiri = pkgs.lomiri.lomiri;
    } // lib.optionalAttrs wcfg.enable {
      lomiri-system-settings = pkgs.lomiri.lomiri-system-settings;
      wallpapersPlugin = pkgs.lomiri.lomiri-system-settings.fp6WallpapersPlugin;
    };
  };
}
