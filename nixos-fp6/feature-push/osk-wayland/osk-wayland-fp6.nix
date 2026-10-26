# On-screen keyboard for Wayland-native text-input clients (Qt6 Morph Browser,
# GTK3 Firefox) in the Lomiri GPU session. See
# results/feature-push-20261024/osk-wayland/README.md for the evidence.
#
# Root cause (two independent breaks, both confirmed on the phone):
#  1. maliit-keyboard runs with MALIIT_FORCE_DBUS_CONNECTION=1 (needed by the
#     Qt5 Ubuntu Touch apps, whose Maliit input-context plugin talks D-Bus).
#     maliit-framework's createConnection() then builds ONLY the D-Bus
#     connection and never binds zwp_input_method_v1. Mir (with
#     mir-input-method-v1.patch) does route text-input v1/v2/v3 to an
#     input-method-v1 client, but there was none: text_input enable went
#     nowhere. With the keyboard started in Wayland mode the full chain
#     worked (zwp_text_input_v2.enable -> zwp_input_method_v1.activate ->
#     Maliit input panel shown), but then the D-Bus side is gone.
#  2. Morph (Qt 6.11) inherits QT_IM_MODULE=maliit from the session. There is
#     no Qt6 Maliit plugin, and Qt6's Wayland QPA treats any QT_IM_MODULE other
#     than "wayland" as a request for a client-side input context, so Morph
#     never creates a text-input object at all (only the compose plugin loads).
#
# Fix:
#  * maliit-hybrid-dbus-wayland-connection.patch: a HybridInputContextConnection
#    (D-Bus server + zwp_input_method_v1 in one MInputContextConnection),
#    selected by MALIIT_HYBRID_CONNECTION=1. Wayland events use a reserved
#    connection id; outbound text goes to whichever client is active. Without
#    the variable maliit behaves exactly as before.
#  * lomiri-keyboard user unit: MALIIT_HYBRID_CONNECTION=1 (the existing
#    MALIIT_FORCE_DBUS_CONNECTION=1 stays and still means "D-Bus only" when
#    the hybrid variable is absent).
#  * Morph/webapp-container wrapper: QT_IM_MODULE unset, so Qt6 uses its
#    built-in text-input (v2 on this Mir). Firefox already has
#    GTK_IM_MODULE=wayland (text-input-v3), see ../lomiri/firefox-lomiri-fp6.nix.
# Landscape sizing patches from ../lomiri-polish are untouched (they patch
# maliit-keyboard and lomiri, this patches maliit-framework only).
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.oskWayland;
in {
  options.hardware.fairphone6.oskWayland = {
    enable = lib.mkEnableOption ''
      the Maliit keyboard for Wayland text-input clients (Qt6 Morph, GTK
      Firefox) next to the D-Bus Maliit path used by the Qt5 apps'';
  };

  config = lib.mkIf cfg.enable {
    # Morph aborted on the first main-frame navigation (typing a URL/search
    # and pressing Enter): onNavigationRequested changed the profile user
    # agent synchronously, and QtWebEngine 6.11's Chromium CHECKs
    # (!in_sync_check_ in InterceptNavigationThrottle). Not an OSK/Mir problem,
    # but it is what made "search crashes the app" once typing worked.
    # mkBefore: patch the real package before morphDpiFix wraps it.
    nixpkgs.overlays = lib.mkMerge [ (lib.mkBefore [ (final: prev: {
      lomiri-qt6 = prev.lomiri-qt6.overrideScope (_lfinal: lprev: {
        morph-browser = lprev.morph-browser.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./morph-browser-defer-user-agent-change.patch ];
          passthru = (old.passthru or { }) // { fp6DeferUserAgent = true; };
        });
      });
    }) ]) (lib.mkAfter [ (final: prev: {
      maliit-framework = prev.maliit-framework.overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [ ./maliit-hybrid-dbus-wayland-connection.patch ];
        passthru = (old.passthru or { }) // { fp6HybridConnection = true; };
      });
    }) ]) ];

    systemd.user.services.lomiri-keyboard.environment.MALIIT_HYBRID_CONNECTION = "1";

    # Qt6 must not look for a (nonexistent) Qt6 Maliit plugin.
    hardware.fairphone6.lomiriGpu.morphDpiFix.unsetEnv = lib.mkOptionDefault [ "QT_IM_MODULE" ];

    system.build.fp6-osk-maliit-framework = pkgs.maliit-framework;
    system.build.fp6-osk-morph-browser = pkgs.lomiri-qt6.morph-browser;

    assertions = [
      {
        assertion = config.hardware.fairphone6.lomiriGpu.morphDpiFix.enable;
        message = "oskWayland unsets QT_IM_MODULE through the Morph wrapper (morphDpiFix); enable it.";
      }
      {
        assertion = pkgs.maliit-framework.passthru.fp6HybridConnection or false;
        message = "oskWayland: the hybrid maliit-framework overlay was not applied.";
      }
      {
        assertion = (pkgs.lomiri-qt6.morph-browser.passthru.fp6Unwrapped or pkgs.lomiri-qt6.morph-browser).passthru.fp6DeferUserAgent or false;
        message = "oskWayland: the Morph user-agent patch was not applied before the Morph wrapper.";
      }
    ];
  };
}
