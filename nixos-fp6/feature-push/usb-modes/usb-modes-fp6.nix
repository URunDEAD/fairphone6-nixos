# USB connection preferences (Android-like "USB preferences", with
# combinable toggles): System Settings > USB, a "Charging via USB · Tap for
# options" notification on connection, a USB indicator / quick-settings tile.
#
#  * fp6-usbmoded (Rust, root, system bus com.fairphone6.UsbModes,
#    interface com.fairphone6.UsbModes1): the one place that decides the
#    gadget's function set. NCM (usb0) is linked/unlinked in configfs here,
#    MTP (fp6-mtp.service) and ADB (fp6-adbd.service, only with Developer
#    mode on) are started/stopped; every change is a planned sequence with
#    rollback in reverse order on failure, serialized on
#    /run/fp6-adb-gadget.lock with fp6-adb-gadget, fp6-mtp and
#    fp6-usb-drived. No function left = UDC unbound (charging only).
#    Watches typec/power_supply/udc uevents: plug -> the user's "default
#    when connected" (build default / charge only / last used / ask);
#    unplug -> back to the idle set. Power role: UCSI PR_Swap through
#    /sys/class/typec/port0/power_role, offered only with a PD partner.
#    Callers: D-Bus policy (phone user + root), uid check, polkit
#    com.fairphone6.usb-modes.change. Data functions are only turned ON
#    while the session agent reports the lock screen inactive.
#  * fp6-usbmodes-agent (user): lock state -> helper, GSettings
#    com.fairphone6.usb-modes (the page) -> helper, notifications, indicator.
#
# SAFETY (developer builds): keepFunctions = [ "ncm" ] keeps usb0
# (172.16.42.1) linked no matter what the page or the policy asks. Builds
# for users: keepFunctions = [ ]; defaultFunctions = [ ] (charge only).
# Restoring USB network if it was removed (keepFunctions = [ ] builds):
# replug with "build default" mode, `sudo fp6-usbmodes-cli set ncm`,
# `sudo fp6-usbmoded --restore-network` (no D-Bus needed),
# `sudo systemctl restart fp6-usbmoded` (re-applies the defaults only
# after a reboot), or reboot. See results/feature-push-20261024/usb-modes/README.md.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.usbModes;
  pkg = pkgs.callPackage ./package.nix { inherit (pkgs.lomiri) lomiri-url-dispatcher; };
  uid = config.users.users.${cfg.user}.uid;
  mtp = config.services.fp6Mtp.enable or false;
  adb = config.hardware.fairphone6.adb.enable or false;
  funcs = lib.types.listOf (lib.types.enum [ "ncm" "mtp" "adb" ]);
  helperArgs = lib.escapeShellArgs ([
    "--uid" (toString uid)
    "--default" (lib.concatStringsSep "," cfg.defaultFunctions)
    "--keep" (lib.concatStringsSep "," cfg.keepFunctions)
    "--systemctl" "${config.systemd.package}/bin/systemctl"
  ] ++ lib.optionals mtp [ "--mtp-unit" "fp6-mtp.service" ]
    ++ lib.optionals adb [ "--adb-unit" "fp6-adbd.service" ]);

  dbusConf = pkgs.writeTextDir "share/dbus-1/system.d/com.fairphone6.UsbModes.conf" ''
    <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
      <policy user="root">
        <allow own="com.fairphone6.UsbModes"/>
        <allow send_destination="com.fairphone6.UsbModes"/>
      </policy>
      <policy user="${cfg.user}">
        <allow send_destination="com.fairphone6.UsbModes" send_interface="com.fairphone6.UsbModes1"/>
        <allow send_destination="com.fairphone6.UsbModes" send_interface="org.freedesktop.DBus.Introspectable"/>
        <allow send_destination="com.fairphone6.UsbModes" send_interface="org.freedesktop.DBus.Peer"/>
        <allow send_destination="com.fairphone6.UsbModes" send_interface="org.freedesktop.DBus.Properties"/>
      </policy>
    </busconfig>
  '';
  polkitAction = pkgs.writeTextDir "share/polkit-1/actions/com.fairphone6.usb-modes.policy" ''
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
    <policyconfig>
      <action id="com.fairphone6.usb-modes.change">
        <description>Change how the USB connection is used</description>
        <message>Changing the USB mode requires the phone's user</message>
        <defaults>
          <allow_any>no</allow_any>
          <allow_inactive>no</allow_inactive>
          <allow_active>yes</allow_active>
        </defaults>
      </action>
    </policyconfig>
  '';
in
{
  options.hardware.fairphone6.usbModes = {
    enable = lib.mkEnableOption "USB connection preferences (System Settings > USB, connection notification)";
    user = lib.mkOption {
      type = lib.types.str;
      default = "user";
      description = "The phone account allowed to change USB modes.";
    };
    defaultFunctions = lib.mkOption {
      type = funcs;
      default = [ "ncm" ];
      description = ''
        Functions at boot and on a new connection while the user's "When a
        computer is connected" is "build default". Applied without unlock
        (the builder's choice, like the stage-1 gadget). [ ] = charge only.
      '';
    };
    keepFunctions = lib.mkOption {
      type = funcs;
      default = [ ];
      example = [ "ncm" ];
      description = ''
        Functions never removed, whatever the page or the policy asks.
        Developer builds keep [ "ncm" ] so SSH over USB (172.16.42.1)
        always works.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ pkg.core polkitAction ];
    environment.pathsToLink = [ "/share/ayatana" ];
    services.dbus.packages = [ dbusConf ];
    security.polkit.enable = true;
    # Lomiri runs in user@.service, outside a logind session: allow_active
    # never matches (see adb-fp6.nix); grant the phone user.
    security.polkit.extraConfig = ''
      polkit.addRule(function(action, subject) {
        if (action.id == "com.fairphone6.usb-modes.change" && subject.user == "${cfg.user}") {
          return polkit.Result.YES;
        }
      });
    '';

    nixpkgs.overlays = [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri-system-settings = lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ pkg.lssPlugin ];
        });
      });
    }) ];

    systemd.services.fp6-usbmoded = {
      description = "USB modes helper (gadget functions, USB-C power role)";
      wantedBy = [ "multi-user.target" ];
      # After the boot-time gadget fixups (ADB sync / Developer mode adbd)
      # so the boot defaults see the final gadget.
      after = [ "systemd-tmpfiles-setup.service" "fp6-adb-gadget-sync.service" "fp6-adbd.service" "dbus.service" ];
      wants = [ "dbus.service" ];
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.UsbModes";
        ExecStart = "${pkg.core}/bin/fp6-usbmoded ${helperArgs}";
        Restart = "on-failure";
        RestartSec = 3;
        StateDirectory = "fp6-usb-modes";
        StateDirectoryMode = "0700";
        RuntimeDirectory = "fp6-usb-modes";
        RuntimeDirectoryMode = "0755";
        # Keep the "booted" marker: a restart adopts the gadget as it is.
        RuntimeDirectoryPreserve = "yes";
        TimeoutStopSec = 15;
        # root: configfs/UDC, typec power_role, systemctl for fp6-mtp/fp6-adbd.
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ReadWritePaths = [ "/sys/kernel/config" "/sys/class/typec" "/sys/devices" "/run" ];
        PrivateTmp = true;
        # Netlink uevents are only delivered in the initial network namespace.
        RestrictAddressFamilies = [ "AF_UNIX" "AF_NETLINK" ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        ProtectClock = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" ];
        UMask = "0022";
      };
    };

    systemd.user.services.fp6-usbmodes-agent = {
      description = "USB modes: lock state, settings bridge, connection notification and indicator";
      wantedBy = [ "graphical-session.target" "ayatana-indicators.target" "lomiri-indicators.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      unitConfig.ConditionUser = cfg.user;
      serviceConfig = {
        ExecStart = "${pkg.core}/bin/fp6-usbmodes-agent";
        Restart = "on-failure";
        RestartSec = 5;
        Nice = 5;
      };
    };

    system.build.fp6-usb-modes = pkg;
    assertions = [
      {
        assertion = config.users.users ? ${cfg.user} && uid != null && uid != 0;
        message = "hardware.fairphone6.usbModes.user must be an existing non-root account with a fixed uid.";
      }
      {
        assertion = lib.all (f: f == "ncm" || (f == "mtp" && mtp) || (f == "adb" && adb)) (cfg.defaultFunctions ++ cfg.keepFunctions);
        message = "hardware.fairphone6.usbModes: default/keep functions must be built (mtp needs services.fp6Mtp, adb the adb module).";
      }
      {
        assertion = !(lib.elem "adb" (cfg.defaultFunctions ++ cfg.keepFunctions));
        message = "hardware.fairphone6.usbModes: ADB follows Developer mode; it cannot be a default.";
      }
    ];
  };
}
