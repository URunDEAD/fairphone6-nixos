# USB Drive (DriveDroid-like): the phone shares an ISO/IMG file as a USB
# CD-ROM or disk to the computer on the USB cable.
#
#  * App (Lomiri, QML + one-class Qt shim + Rust backend): image library
#    (~/Images by default) with ISO 9660 / El Torito / hybrid / MBR / GPT /
#    FAT detection, SHA-256 verification (entered sum or a .sha256 /
#    SHA256SUMS / CHECKSUM file next to the image), decompression of
#    .xz/.gz/.zst/.bz2, blank images (sparse; FAT32 with MBR or exFAT), and
#    curated downloads (downloads.json: project HTTPS hosts only, SHA-256 from
#    the project's own sums file). Drawer folder "Tools".
#  * fp6-usb-drived (Rust, root, D-Bus system service org.fairphone6.UsbDrive,
#    activated on demand): the ONLY component that changes configfs. Adds
#    functions/mass_storage.fp6drive next to NCM (and ADB/MTP if present) in
#    the existing gadget g1 (UDC unbind, link, rebind: ~1 s USB
#    re-enumeration), under the shared gadget lock /run/fp6-adb-gadget.lock
#    used by adb/fp6-adb-gadget and mtp-fp6.nix. Exclusive mode removes the
#    other links while hosting; they come back on Stop, when the cable is
#    unplugged, after exclusiveMaxMinutes, when the service stops, and (from
#    the state file in /run) after a crash (ExecStopPost --recover).
#    Callers: D-Bus policy (phone user + root), uid check, polkit action
#    org.fairphone6.usb-drive.host. Paths: allowed folders only (imageDirs),
#    openat2 without symlinks, caller-owned regular single-link files; the
#    kernel gets /proc/self/fd/N of the helper's own open file.
#  * fp6-usb-drive-agent (user service): indicator "USB Drive" (only while
#    hosting; Stop sharing / Open) and notification bubbles.
# Nothing runs at boot except the idle agent; the gadget is unchanged until
# the user starts sharing. See results/feature-push-20261024/usb-drive/README.md.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.usbDrive;
  pkg = pkgs.callPackage ./package.nix { };
  core = pkg.core;
  uid = config.users.users.${cfg.user}.uid;

  helperArgs = lib.escapeShellArgs ([
    "--uid" (toString uid)
    "--exclusive-max-minutes" (toString cfg.exclusiveMaxMinutes)
    "--gadget" "/sys/kernel/config/usb_gadget/g1"
    "--lock" "/run/fp6-adb-gadget.lock"
    "--state" "/run/fp6-usb-drive/state.json"
  ] ++ lib.concatMap (d: [ "--allow" d ]) cfg.imageDirs);

  dbusConf = pkgs.writeTextDir "share/dbus-1/system.d/org.fairphone6.UsbDrive.conf" ''
    <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
      <policy user="root">
        <allow own="org.fairphone6.UsbDrive"/>
        <allow send_destination="org.fairphone6.UsbDrive"/>
      </policy>
      <policy user="${cfg.user}">
        <allow send_destination="org.fairphone6.UsbDrive" send_interface="org.fairphone6.UsbDrive1"/>
        <allow send_destination="org.fairphone6.UsbDrive" send_interface="org.freedesktop.DBus.Introspectable"/>
        <allow send_destination="org.fairphone6.UsbDrive" send_interface="org.freedesktop.DBus.Peer"/>
        <allow send_destination="org.fairphone6.UsbDrive" send_interface="org.freedesktop.DBus.Properties"/>
      </policy>
    </busconfig>
  '';
  dbusService = pkgs.writeTextDir "share/dbus-1/system-services/org.fairphone6.UsbDrive.service" ''
    [D-BUS Service]
    Name=org.fairphone6.UsbDrive
    Exec=${pkgs.coreutils}/bin/false
    User=root
    SystemdService=fp6-usb-drived.service
  '';
  polkitAction = pkgs.writeTextDir "share/polkit-1/actions/org.fairphone6.usb-drive.policy" ''
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
    <policyconfig>
      <action id="org.fairphone6.usb-drive.host">
        <description>Share a disk image over USB</description>
        <message>Sharing an image as a USB drive requires the phone's user</message>
        <defaults>
          <allow_any>no</allow_any>
          <allow_inactive>no</allow_inactive>
          <allow_active>yes</allow_active>
        </defaults>
      </action>
    </policyconfig>
  '';

  indicatorFile = pkgs.writeTextDir "share/ayatana/indicators/com.fairphone6.indicator.usbdrive" ''
    [Indicator Service]
    Name=fp6-usb-drive-indicator
    ObjectPath=/com/fairphone6/indicator/usbdrive
    Position=72

    [phone]
    ObjectPath=/com/fairphone6/indicator/usbdrive/phone

    [phone_greeter]
    ObjectPath=/com/fairphone6/indicator/usbdrive/phone_greeter

    [desktop]
    ObjectPath=/com/fairphone6/indicator/usbdrive/desktop

    [desktop_greeter]
    ObjectPath=/com/fairphone6/indicator/usbdrive/desktop_greeter
  '';

  desktopEntry = pkgs.writeTextDir "share/applications/fp6-usb-drive.desktop" (''
    [Desktop Entry]
    Type=Application
    Name=USB Drive
    Comment=Boot or install a computer from an ISO or disk image on the phone
    Keywords=usb;iso;img;drivedroid;boot;install;cdrom;disk;image;
    Exec=${pkg}/bin/fp6-usb-drive
    Icon=${pkg}/share/fp6-usb-drive/fp6-usb-drive.png
    Terminal=false
    Categories=Utility;System;
    X-Lomiri-Touch=true
    X-Lomiri-Default-Department-ID=accessories
  '' + lib.concatStrings (lib.mapAttrsToList (k: v: "${k}=${v}\n") cfg.desktopExtraKeys));
in
{
  options.hardware.fairphone6.usbDrive = {
    enable = lib.mkEnableOption "the USB Drive app (share ISO/IMG files as a USB CD-ROM or disk)";
    user = lib.mkOption {
      type = lib.types.str;
      default = "user";
      description = "The phone account allowed to share images.";
    };
    imageDirs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "/home/${cfg.user}/Images" ];
      defaultText = lib.literalExpression ''[ "/home/''${user}/Images" ]'';
      description = ''
        Folders the helper accepts images from (no symlinks anywhere in the
        path). The app's library folder setting may point elsewhere, but only
        files inside these folders can be shared.
      '';
    };
    exclusiveMaxMinutes = lib.mkOption {
      type = lib.types.ints.positive;
      default = 240;
      description = "Exclusive mode (no USB network/ADB) ends by itself after this time.";
    };
    desktopExtraKeys = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { "X-Lomiri-Folder" = "Tools"; };
      description = "Extra keys for the desktop entry (app drawer folder).";
    };
    apparmor = lib.mkOption {
      type = lib.types.enum [ "disable" "complain" "enforce" ];
      default = if config.hardware.fairphone6.apparmor.enable or false then "complain" else "disable";
      defaultText = lib.literalExpression ''"complain" when hardware.fairphone6.apparmor.enable'';
      description = "Mode of the fp6-usb-drive app profile (complain until exercised live).";
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      environment.systemPackages = [ pkg desktopEntry indicatorFile polkitAction ];
      environment.pathsToLink = [ "/share/ayatana" ];
      environment.etc."fp6-usb-drive/downloads.json".source = ./downloads.json;
      services.dbus.packages = [ dbusConf dbusService ];
      security.polkit.enable = true;
      # Lomiri apps run in user@.service, outside a logind session, so
      # allow_active never matches (see adb-fp6.nix); grant the phone user.
      security.polkit.extraConfig = ''
        polkit.addRule(function(action, subject) {
          if (action.id == "org.fairphone6.usb-drive.host" && subject.user == "${cfg.user}") {
            return polkit.Result.YES;
          }
        });
      '';
      systemd.tmpfiles.rules = map (d: "d ${d} 0750 ${cfg.user} ${config.users.users.${cfg.user}.group} -") cfg.imageDirs;

      systemd.services.fp6-usb-drived = {
        description = "USB Drive helper (mass storage gadget function)";
        # D-Bus activated only; nothing at boot.
        serviceConfig = {
          Type = "dbus";
          BusName = "org.fairphone6.UsbDrive";
          ExecStart = "${core}/bin/fp6-usb-drived ${helperArgs}";
          # Restores the gadget from /run/fp6-usb-drive/state.json if the
          # daemon died while hosting (no-op otherwise).
          ExecStopPost = "${core}/bin/fp6-usb-drived --recover ${helperArgs}";
          Restart = "on-failure";
          RestartSec = 2;
          TimeoutStopSec = 45;
          RuntimeDirectory = "fp6-usb-drive";
          RuntimeDirectoryPreserve = "yes";
          RuntimeDirectoryMode = "0700";
          # root: configfs, UDC, reading the user's image folder, /proc/<kthread>/io.
          CapabilityBoundingSet = [ "CAP_DAC_OVERRIDE" "CAP_DAC_READ_SEARCH" "CAP_SYS_PTRACE" ];
          NoNewPrivileges = true;
          ProtectSystem = "strict";
          ProtectHome = "read-only";
          ReadWritePaths = [ "/sys/kernel/config" "/run" ] ++ map (d: "-${d}") cfg.imageDirs;
          PrivateTmp = true;
          PrivateNetwork = true;
          RestrictAddressFamilies = [ "AF_UNIX" ];
          RestrictNamespaces = true;
          RestrictRealtime = true;
          LockPersonality = true;
          MemoryDenyWriteExecute = true;
          ProtectClock = true;
          ProtectHostname = true;
          ProtectKernelLogs = true;
          ProtectControlGroups = true;
          SystemCallArchitectures = "native";
          SystemCallFilter = [ "@system-service" ];
          UMask = "0077";
        };
      };

      systemd.user.services.fp6-usb-drive-agent = {
        description = "USB Drive status indicator and notifications";
        wantedBy = [ "graphical-session.target" "ayatana-indicators.target" "lomiri-indicators.target" ];
        partOf = [ "graphical-session.target" ];
        after = [ "graphical-session.target" ];
        unitConfig.ConditionUser = cfg.user;
        serviceConfig = {
          ExecStart = "${core}/bin/fp6-usb-drive-agent";
          Restart = "on-failure";
          RestartSec = 5;
          Nice = 5;
        };
      };

      system.build.fp6-usb-drive = pkg;
      assertions = [{
        assertion = config.users.users ? ${cfg.user} && uid != null && uid != 0;
        message = "hardware.fairphone6.usbDrive.user must be an existing non-root account with a fixed uid.";
      }];
    }

    # The "Tools" drawer folder and its icon are set up by sensor-apps
    # (desktopExtraKeys there); this entry only joins it.

    (lib.mkIf (cfg.apparmor != "disable") {
      security.apparmor.policies.fp6-usb-drive = {
        state = cfg.apparmor;
        path = ./fp6-usb-drive.apparmor;
      };
      system.checks = [
        (pkgs.runCommand "fp6-usb-drive-apparmor-compile" { } ''
          ${pkgs.apparmor-parser}/bin/apparmor_parser --config-file /dev/null -Q -K \
            -I ${config.environment.etc."apparmor.d".source} \
            ${lib.concatMapStringsSep " " (p: "-I ${p}/etc/apparmor.d") config.security.apparmor.packages} \
            ${./fp6-usb-drive.apparmor} 2>&1 | { grep -v 'Cache read/write disabled' || true; }
          test "''${PIPESTATUS[0]}" = 0
          touch $out
        '')
      ];
    })
  ]);
}
