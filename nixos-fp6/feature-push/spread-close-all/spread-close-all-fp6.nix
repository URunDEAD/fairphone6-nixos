# FP6 close all apps from the app switcher (Lomiri's spread), 2026-10-27.
# Design, previews and the phone checklist:
# results/feature-push-20261024/spread-close-all/README.md
#
#  * Gesture: in the spread, a left-to-right drag that starts while the list
#    is already at its first app (always the case when all cards fit) pulls
#    the closable cards right and fades them; past the threshold (40 % of the
#    width, 16-30 gu) "Release to close all apps" appears with a haptic tick
#    (UITK Haptics -> hfd, misc/haptics-fp6.nix adds the FP6's +15 ms).
#    Release there: the cards fly out, the apps are closed, the spread closes
#    and the shell goes home (showHome(): the app drawer, like the home
#    button). Release before: spring back. A drag that starts anywhere else
#    only scrolls, even when it reaches the start.
#  * "Clear all" button in the spread's bottom margin (same close path).
#  * Close path = the per-card swipe-up close (appDelegate.close() ->
#    Window::close -> surface close request, qtmir force-closes after 3 s;
#    a window without surface -> Application::close); anything still listed
#    after 2 s gets ApplicationManager.stopApplication().
#  * Kept open: the dialer while a call is in progress (callManager.hasCalls);
#    with "Keep 'ready' apps when closing all" (default ON) the apps in App
#    Launch's keep-ready and prelaunch lists (com.fairphone6.app-ready). The
#    shell, the OSK, indicators and services are not spread windows.
#  * Settings > Gesture Shortcuts > App switcher (section in the gestures
#    page, shown when the schema is installed): com.fairphone6.app-switcher
#    close-all-gesture (b, true), close-all-keep-ready (b, true).
#
# Lomiri patch (lomiri-spread-close-all.patch + qml/Fp6CloseAll.qml ->
# qml/Stage/Spread/): FloatingFlickable drag hooks, Stage.qml (overlay in the
# Spread block, card x/opacity in the delegates' spread state, a signal),
# Shell.qml (signal -> showHome). Applied in postPatch at lib.mkOrder 3300:
# after every other module's Lomiri patches (app-ready 2600 Stage.qml,
# multiwindow 2200, quick-settings 3100 postPatch, lockscreen-polish 3200
# Stage.qml wallpaper). Regenerate with test/mkpatch.sh if their context moves.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.spreadCloseAll;
  pkg = pkgs.callPackage ./package.nix { };
in {
  options.hardware.fairphone6.spreadCloseAll.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = ''
      Close all apps from the Lomiri app switcher: overscroll gesture at the
      start of the list and a "Clear all" button; settings in
      Settings > Gesture Shortcuts > App switcher.
    '';
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = lib.mkOrder 3300 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          # Fp6CloseAll.qml reads com.fairphone6.app-switcher (wrapGAppsHook adds the schema).
          buildInputs = (old.buildInputs or [ ]) ++ [ pkg.schema ];
          postPatch = (old.postPatch or "") + ''
            # FP6 spread close all (after every other module's Lomiri edits)
            patch -p1 --no-backup-if-mismatch < ${./lomiri-spread-close-all.patch}
            install -m644 ${./qml/Fp6CloseAll.qml} qml/Stage/Spread/Fp6CloseAll.qml
          '';
          passthru = (old.passthru or { }) // { fp6SpreadCloseAll = true; };
        });
        lomiri-system-settings = lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ pkg.lssPlugin ];
        });
      });
    }) ];

    environment.systemPackages = [ pkg.schema ];

    system.build.fp6-spread-close-all = {
      inherit (pkg) schema lssPlugin;
      lomiri = pkgs.lomiri.lomiri;
    };

    assertions = [
      { assertion = config.hardware.fairphone6.gestures.enable or false;
        message = "spreadCloseAll's settings are a section of the Gesture Shortcuts page (hardware.fairphone6.gestures.enable)."; }
    ];
  };
}
