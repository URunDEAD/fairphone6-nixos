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
#
# Making room for the keyboard (resizeStagedApps, 2026-10-26; notes in
# results/feature-push-20261024/osk-resize/README.md). Lomiri itself never
# resized staged (phone) windows for the OSK. Qt5 Maliit D-Bus clients get the
# keyboard rectangle and shrink their own content (UITK anchorToKeyboard).
# Wayland text-input clients (Firefox/GTK3, GTK4 apps such as Fragments, Qt6
# Morph) get no panel geometry from Mir (text-input-v3 has no such event, Mir
# sends no text-input-v2 input_panel_state). What did change their size was
# Mir: the fp6 input panel is an "attached" window with an exclusive rect and
# an output (set_toplevel), and on every keyboard show/hide/resize miral
# recomputes the application zone and re-places every Mir-maximized window
# (all staged windows since lomiri-staged-maximized) itself, overriding
# Lomiri's geometry: Firefox ended ~24 px short (its bottom toolbar stayed
# under the keyboard), Qt5 apps were shrunk twice (the terminal showed
# wallpaper between its content and the keyboard), and with only the
# exclusive rect removed, windows were reset to the full 828 px (under the
# shell's top panel) 10 ms after Lomiri had resized them.
#  * qtmir-input-panel-detached.patch: the input panel is "restored" instead
#    of "attached" and gets no exclusive rect, attached edges or output, so
#    Mir never re-places windows for the keyboard; Lomiri (which draws the
#    panel) owns the layout.
#  * maliit-publish-active-client.patch: the hybrid connection writes
#    $XDG_RUNTIME_DIR/maliit-active-client/state ("wayland"/"dbus"/"none")
#    whenever the active client changes (before the panel is shown).
#  * lomiri-staged-osk-resize.patch: Utils.InputMethodClient watches that
#    file; while the keyboard is shown for a Wayland client, Stage.qml asks for
#    the focused staged window's height minus the keyboard (shell
#    coordinates). D-Bus (Qt5) clients keep the full height and resize their
#    own content, as on Ubuntu Touch.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.oskWayland;
in {
  options.hardware.fairphone6.oskWayland = {
    enable = lib.mkEnableOption ''
      the Maliit keyboard for Wayland text-input clients (Qt6 Morph, GTK
      Firefox) next to the D-Bus Maliit path used by the Qt5 apps'';
    resizeStagedApps = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Shrink the focused staged (phone) window above the on-screen keyboard
        while the keyboard serves a Wayland text-input client (GTK, Firefox,
        Qt6), which cannot learn the keyboard size itself. Maliit D-Bus (Qt5)
        clients keep resizing themselves.
      '';
    };
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
        patches = (old.patches or [ ]) ++ [ ./maliit-hybrid-dbus-wayland-connection.patch ]
          ++ lib.optional cfg.resizeStagedApps ./maliit-publish-active-client.patch;
        passthru = (old.passthru or { }) // { fp6HybridConnection = true; };
      });
    }) ])
    # After lomiri-polish (mkOrder 2000) and gtk-lomiri (mkAfter): the patch
    # was made on top of their Stage.qml / Utils plugin changes.
    (lib.mkIf cfg.resizeStagedApps (lib.mkOrder 2100 [ (final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri = lprev.lomiri.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-staged-osk-resize.patch ];
          passthru = (old.passthru or { }) // { fp6StagedOskResize = true; };
        });
        qtmir = lprev.qtmir.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./qtmir-input-panel-detached.patch ];
        });
      });
    }) ])) ];

    systemd.user.services.lomiri-keyboard.environment.MALIIT_HYBRID_CONNECTION = "1";

    # Qt6 must not look for a (nonexistent) Qt6 Maliit plugin.
    hardware.fairphone6.lomiriGpu.morphDpiFix.unsetEnv = lib.mkOptionDefault [ "QT_IM_MODULE" ];

    system.build.fp6-osk-maliit-framework = pkgs.maliit-framework;
    system.build.fp6-osk-morph-browser = pkgs.lomiri-qt6.morph-browser;
    system.build.fp6-osk-lomiri = pkgs.lomiri.lomiri;
    system.build.fp6-osk-qtmir = pkgs.lomiri.qtmir;

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
