# Inline replies for any app's notifications on Lomiri (FP6), 2026-10-28.
# Design, tests and the phone checklist: ./README.md
#
#  * lomiri-notifications (lomiri-notifications-inline-reply.patch, C++):
#    GetCapabilities lists "inline-reply" (as KDE Plasma); a Notify whose
#    actions contain "inline-reply" gets notification.canReply, the action
#    label in the hint x-fp6-inline-reply (only the server sets it) and is not
#    a button; Notification.reply(text) emits
#    org.freedesktop.Notifications.NotificationReplied(id, text) and closes it.
#    holdForReply() keeps the bubble up while typing. A replyable bubble that
#    times out is "parked": still open for its sender (no NotificationClosed)
#    so the panel can reply later, until replied, dismissed in the panel,
#    closed or replaced by its sender, its sender leaves the bus, or more
#    than 32 are parked. Model.fp6CanReply/fp6Reply/fp6Release/fp6ParkNow
#    and the signal fp6ReplyableChanged are for the shell.
#  * Lomiri (lomiri-inline-reply.patch, QML): Reply button + field in the
#    bubble (Notification.qml), Reply in the pulled-down panel's notification
#    cards (quick-settings' QsNotifications.qml via Panel/ControlCentre), the
#    lock state and unlock request from Shell.qml. Never typed into while the
#    lock screen is up: Reply asks for the passcode (greeter
#    notifyUserRequestedApp) and the field opens after the unlock; locking
#    drops a draft. Text stays hidden per "hide notification content while
#    locked" (privacyMode / hideContent are unchanged). DND "hide popups"
#    parks replyable bubbles instead of closing them.
#  * The SMS quick reply (telephony-service, messaging indicator) is
#    unchanged. KDE Connect: the daemon (../../kdeconnect) offers inline-reply
#    for peer notifications that carry requestReplyId and sends
#    kdeconnect.notification.reply.
#  * fp6-notify-reply (Rust, test client): sends an inline-reply notification
#    and prints the reply.
#
# Overlay order: lib.mkOrder 3700, after every other Lomiri overlay (highest
# so far: accessibility / edge-flash 3600). The shell patch is applied in
# postPatch, after notifications (2900, Notifications.qml DND), privacy-lock
# (2800, Shell.qml), quick-settings (3100, installs QsNotifications.qml and
# ControlCentre.qml, Panel.qml), spread-close-all (3300) and edge-flash (3600,
# Notification.qml). lomiri-notifications' patch comes after notifications'
# lomiri-notifications-app-name.patch. Regenerate the shell patch with
# test/mkpatch.sh if their context moves.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.inlineReply;
  client = pkgs.callPackage ./package.nix { };
in {
  options.hardware.fairphone6.inlineReply.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = ''
      Freedesktop/KDE inline replies ("inline-reply" capability,
      NotificationReplied) in the Lomiri notification server, with a reply
      field in the bubbles and the pulled-down panel (after unlock only).
    '';
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = lib.mkOrder 3700 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri-notifications = lprev.lomiri-notifications.overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./lomiri-notifications-inline-reply.patch ];
          passthru = (old.passthru or { }) // { fp6InlineReply = true; };
        });
        lomiri = lprev.lomiri.overrideAttrs (old: {
          postPatch = (old.postPatch or "") + ''
            # FP6 inline reply (after every other module's Lomiri edits)
            patch -p1 --no-backup-if-mismatch < ${./lomiri-inline-reply.patch}
          '';
          passthru = (old.passthru or { }) // { fp6InlineReply = true; };
        });
      });
    }) ];

    environment.systemPackages = [ client ];

    system.build.fp6-inline-reply = {
      inherit client;
      lomiri = pkgs.lomiri.lomiri;
      lomiri-notifications = pkgs.lomiri.lomiri-notifications;
    };

    assertions = [
      { assertion = config.hardware.fairphone6.notifications.enable or false;
        message = "inlineReply patches on top of the notifications module (app-name patch, DND Notifications.qml)."; }
      { assertion = config.hardware.fairphone6.quickSettings.enable or false;
        message = "inlineReply's panel reply is in quick-settings' notification cards (quickSettings.enable)."; }
    ];
  };
}
