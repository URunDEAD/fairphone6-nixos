# FP6 ADB hardening: Developer mode switch, RSA key authorization, user shell.
#
# Before: mobile-nixos started adbd in stage 1 and again in stage 2
# (modules/adb.nix), as root, with ro.adb.secure unset, so `adb shell` from any
# computer on the USB-C port was uid 0 with no authorization.
#
# Now (like Ubuntu Touch):
#  * ADB is off by default. System Settings > About > Developer mode turns it on
#    and off. lomiri-system-settings 1.3.2 talks to usb_moded for that
#    (com.meego.usb_moded get_modes/get_config/set_config/set_mode); the
#    fp6-developer-mode D-Bus service implements just that subset and starts or
#    stops fp6-adbd.service. Setting it needs polkit
#    org.fairphone6.developer-mode (granted to the phone user only). The page itself
#    only enables the switch when a passcode/passphrase is set. The choice
#    persists in /var/lib/fp6-adb/developer-mode.
#  * adbd-fp6 (adbd-fp6-auth-user.patch): RSA authorization always on; keys in
#    /etc/adbd/adb_keys (authorizedKeys below) and /var/lib/adbd/adb_keys.
#    Unknown keys go to fp6-adb-auth-prompt in the Lomiri session: a snap
#    decision "Allow USB debugging?" with the key fingerprint, only while the
#    phone is unlocked. adbd drops to `user` before touching USB, so shell,
#    push and pull run as `user`; root is `sudo` with the password.
#  * USB networking never depends on ADB: off = NCM only in the gadget; on =
#    NCM + ffs.adb (fp6-adb-gadget adds/removes the function and rebinds the
#    UDC, a ~1 s re-enumeration). stage1 = false (default) also removes the
#    root adbd from the initrd.
# See results/feature-push-20261024/adb/README.md.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.adb;

  adbdFp6 = pkgs.adbd.overrideAttrs (old: {
    name = "adbd-fp6";
    patches = (old.patches or [ ]) ++ [ ./adbd-fp6-auth-user.patch ];
  });

  python = pkgs.python3.withPackages (ps: [ ps.dbus-python ps.pygobject3 ]);

  gadgetTool = pkgs.writeShellApplication {
    name = "fp6-adb-gadget";
    runtimeInputs = with pkgs; [ coreutils util-linux kmod ];
    text = builtins.readFile ./fp6-adb-gadget.sh;
  };

  developerMode = pkgs.writeScriptBin "fp6-developer-mode" ''
    #!${python}/bin/python3
    ${builtins.readFile ./fp6-developer-mode.py}
  '';

  authPrompt = pkgs.writeScriptBin "fp6-adb-auth-prompt" ''
    #!${python}/bin/python3
    ${builtins.readFile ./fp6-adb-auth-prompt.py}
  '';

  # D-Bus policy + activation + polkit action. Deliberately NOT named
  # usb_moded.conf: lomiri-system-settings shows its USB mode page (charging
  # only / MTP / tethering) when that file exists, and those modes would take
  # the NCM link away.
  dbusPackage = pkgs.writeTextFile {
    name = "fp6-developer-mode-dbus";
    destination = "/share/dbus-1/system.d/fp6-developer-mode.conf";
    text = ''
      <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
       "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
      <busconfig>
        <policy user="root">
          <allow own="com.meego.usb_moded"/>
          <allow send_destination="com.meego.usb_moded"/>
        </policy>
        <policy context="default">
          <allow send_destination="com.meego.usb_moded" send_interface="org.freedesktop.DBus.Introspectable"/>
          <allow send_destination="com.meego.usb_moded" send_interface="org.freedesktop.DBus.Peer"/>
          <allow send_destination="com.meego.usb_moded" send_interface="com.meego.usb_moded" send_member="get_modes"/>
          <allow send_destination="com.meego.usb_moded" send_interface="com.meego.usb_moded" send_member="get_available_modes"/>
          <allow send_destination="com.meego.usb_moded" send_interface="com.meego.usb_moded" send_member="get_config"/>
          <allow send_destination="com.meego.usb_moded" send_interface="com.meego.usb_moded" send_member="get_target_state"/>
          <allow send_destination="com.meego.usb_moded" send_interface="com.meego.usb_moded" send_member="mode_request"/>
        </policy>
        <policy user="${cfg.user}">
          <allow send_destination="com.meego.usb_moded" send_interface="com.meego.usb_moded" send_member="set_config"/>
          <allow send_destination="com.meego.usb_moded" send_interface="com.meego.usb_moded" send_member="set_mode"/>
        </policy>
      </busconfig>
    '';
  };
  dbusActivation = pkgs.writeTextFile {
    name = "fp6-developer-mode-dbus-service";
    destination = "/share/dbus-1/system-services/com.meego.usb_moded.service";
    text = ''
      [D-BUS Service]
      Name=com.meego.usb_moded
      Exec=${pkgs.coreutils}/bin/false
      User=root
      SystemdService=fp6-developer-mode.service
    '';
  };
  polkitAction = pkgs.writeTextFile {
    name = "fp6-developer-mode-polkit";
    destination = "/share/polkit-1/actions/org.fairphone6.developer-mode.policy";
    text = ''
      <?xml version="1.0" encoding="UTF-8"?>
      <!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
       "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
      <policyconfig>
        <action id="org.fairphone6.developer-mode">
          <description>Turn Developer mode (USB debugging) on or off</description>
          <message>Changing Developer mode requires the phone's active session</message>
          <defaults>
            <allow_any>no</allow_any>
            <allow_inactive>no</allow_inactive>
            <allow_active>yes</allow_active>
          </defaults>
        </action>
      </policyconfig>
    '';
  };

  stateFile = "/var/lib/fp6-adb/developer-mode";
  keysEtc = "adbd/adb_keys";
in {
  options.hardware.fairphone6.adb = {
    enable = lib.mkOption {
      type = lib.types.bool;
      # Importing the module is the switch (one import line in the trial file).
      default = true;
      description = ''
        Developer-mode-gated adbd with RSA key authorization that runs shells
        as an unprivileged user, replacing mobile-nixos' always-on root adbd.
      '';
    };
    user = lib.mkOption {
      type = lib.types.str;
      default = "user";
      description = "Account adbd drops to (adb shell, push, pull).";
    };
    stage1 = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Keep mobile-nixos' root adbd (no authorization) in stage 1 (initrd)
        for boot debugging. Off by default: stage 1 then only brings up NCM
        networking. It is killed at the switch to stage 2 either way.
      '';
    };
    authorizedKeys = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "QAAAAM0muSn7yQCY...= user@laptop" ];
      description = ''
        Contents of ~/.android/adbkey.pub of computers that are always
        allowed (/etc/adbd/adb_keys). Keys approved on the phone go to
        /var/lib/adbd/adb_keys instead.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    mobile.adbd.enable = lib.mkForce cfg.stage1;
    # With stage1 = true, mobile-nixos would also start its stage-2 root adbd.
    systemd.services.adbd = lib.mkIf cfg.stage1 { enable = lib.mkForce false; };

    environment.etc.${keysEtc} = {
      text = lib.concatMapStrings (k: k + "\n") cfg.authorizedKeys;
      mode = "0644";
    };

    systemd.tmpfiles.rules = [
      "d /var/lib/fp6-adb 0755 root root -"
      "d /var/lib/adbd 0700 ${cfg.user} ${config.users.users.${cfg.user}.group} -"
    ];

    services.dbus.packages = [ dbusPackage dbusActivation ];
    environment.systemPackages = [ polkitAction gadgetTool ];
    security.polkit.enable = true;
    # Lomiri and its apps (lomiri-system-settings included) run in
    # user@.service, outside any logind session, so polkit never sees them as
    # "active" (measured: allow_active=yes is denied from a user-manager
    # process). Grant the action to the phone user, like the NetworkManager
    # group rule does for Wi-Fi; other accounts still get "no".
    security.polkit.extraConfig = ''
      polkit.addRule(function(action, subject) {
        if (action.id == "org.fairphone6.developer-mode" && subject.user == "${cfg.user}") {
          return polkit.Result.YES;
        }
      });
    '';

    systemd.services.fp6-adbd = {
      description = "ADB daemon (Developer mode; RSA authorization; shells as ${cfg.user})";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-tmpfiles-setup.service" "fp6-adb-gadget-sync.service" ];
      unitConfig.ConditionPathExists = stateFile;
      environment.ADBD_USER = cfg.user;
      serviceConfig = {
        Type = "simple";
        # adbd creates its socket and drops to ADBD_USER itself (it must
        # initgroups()), so it starts as root without extra sandboxing:
        # `sudo` in adb shell has to keep working.
        ExecStartPre = "${gadgetTool}/bin/fp6-adb-gadget prepare";
        ExecStart = "${adbdFp6}/bin/adbd";
        ExecStartPost = "${gadgetTool}/bin/fp6-adb-gadget bind";
        ExecStopPost = "${gadgetTool}/bin/fp6-adb-gadget unbind";
        RuntimeDirectory = "adbd";
        RuntimeDirectoryMode = "0755";
        Restart = "on-failure";
        RestartSec = 3;
      };
    };

    # Developer mode off at boot: make sure no ADB function (e.g. a stage-1
    # leftover with stage1 = true) is in the gadget and the UDC is bound.
    systemd.services.fp6-adb-gadget-sync = {
      description = "USB gadget without ADB (Developer mode off)";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-tmpfiles-setup.service" ];
      unitConfig = {
        ConditionPathExists = [ "!${stateFile}" "/sys/kernel/config/usb_gadget/g1" ];
      };
      environment.ADBD_USER = cfg.user;
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${gadgetTool}/bin/fp6-adb-gadget sync";
      };
    };

    systemd.services.fp6-developer-mode = {
      description = "Developer mode switch backend (usb_moded API subset)";
      serviceConfig = {
        Type = "dbus";
        BusName = "com.meego.usb_moded";
        ExecStart = "${developerMode}/bin/fp6-developer-mode";
        Environment = [ "FP6_DEVMODE_UNIT=fp6-adbd.service" "FP6_SYSTEMCTL=${config.systemd.package}/bin/systemctl" ];
        ProtectHome = true;
        PrivateTmp = true;
      };
    };

    # The on-phone authorization prompt, in the user's graphical session,
    # whenever adbd's socket exists (Developer mode on).
    systemd.user.paths.fp6-adb-auth-prompt = {
      description = "Watch for adbd's authorization socket";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      unitConfig.ConditionUser = cfg.user;
      pathConfig.PathExists = "/run/adbd/adbd";
    };
    systemd.user.services.fp6-adb-auth-prompt = {
      description = "Allow USB debugging? prompt for adbd";
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      unitConfig.ConditionUser = cfg.user;
      serviceConfig = {
        ExecStart = "${authPrompt}/bin/fp6-adb-auth-prompt";
        Restart = "no";
      };
    };

    system.build.fp6-adbd = adbdFp6;
    system.build.fp6-adb-tools = pkgs.symlinkJoin {
      name = "fp6-adb-tools";
      paths = [ adbdFp6 gadgetTool developerMode authPrompt dbusPackage dbusActivation polkitAction ];
    };

    assertions = [{
      assertion = config.users.users ? ${cfg.user} && config.users.users.${cfg.user}.uid != 0;
      message = "hardware.fairphone6.adb.user must be an existing non-root account.";
    }];
  };
}
