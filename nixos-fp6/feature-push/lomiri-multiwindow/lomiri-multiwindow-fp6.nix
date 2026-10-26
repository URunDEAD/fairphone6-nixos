# Split screen and a floating window for Lomiri's phone ("staged") stage.
# Notes: results/feature-push-20261024/multiwindow/README.md.
#
#  * Entry point: in the spread (swipe from the right edge), press and hold an
#    app card -> "Split screen" / "Floating window" (and, for apps already in
#    the layout, "Swap", "Exit split screen", "Full screen"). A one-line hint
#    under the cards says so until the menu was used once in the session.
#  * Split screen: two apps, top/bottom in portrait and left/right in
#    landscape (re-laid out on rotation). The divider can be dragged (25-75 %);
#    releasing it near an edge closes that side, double-tap swaps the apps.
#    Opening a third app replaces the half that had the focus last.
#  * Floating window: one app above the full-screen app, with a title bar
#    (drag to move, full screen, close) and a resize grip. It is not
#    maximized for Mir (StagedFullscreenPolicy inactive), unlike every other
#    phone-stage window (lomiri-staged-maximized.patch).
#  * On-screen keyboard: in portrait the app you type into (a split half or
#    the floating window) is shown full screen while the keyboard is up; the
#    layout comes back when it closes. Landscape halves keep the split and use
#    the staged OSK inset of osk-wayland/lomiri-staged-osk-resize.patch.
#  * Closing an app of the layout ends that layout, the other app stays.
#  * Nothing changes until the user picks one of the menu entries.
#
# Patch order: on top of gtk-lomiri (mkAfter), lomiri-polish (mkOrder 2000) and
# osk-wayland's lomiri-staged-osk-resize.patch (mkOrder 2100); the Stage.qml
# hunks use that patch's stagedOskInset.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.lomiriMultiWindow;
in {
  options.hardware.fairphone6.lomiriMultiWindow = {
    enable = lib.mkEnableOption "split screen and a floating window in Lomiri's phone stage (spread long-press menu)";
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = lib.mkOrder 2200 [ (final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-phone-multiwindow.patch ];
          passthru = (old.passthru or { }) // { fp6PhoneMultiWindow = true; };
        });
      });
    }) ];

    system.build.fp6-multiwindow-lomiri = pkgs.lomiri.lomiri;

    assertions = [{
      assertion = pkgs.lomiri.lomiri.passthru.fp6StagedOskResize or false;
      message = "lomiriMultiWindow: its Stage.qml patch needs osk-wayland's lomiri-staged-osk-resize.patch (hardware.fairphone6.oskWayland.resizeStagedApps).";
    }];
  };
}
