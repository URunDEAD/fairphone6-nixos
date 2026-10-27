# Storage manager and Backup (feature push 2026-10-24, roadmap priority 2).
# Notes, safety rules and the phone test list:
# results/feature-push-20261024/storage-backup/README.md
#
#  * System Settings > Storage (also opened from About > Storage, whose stock
#    page counts Click/Libertine/Anbox and /userdata): usage by category
#    (system closure vs old versions vs unused store paths, logs, photos and
#    videos, music, documents, downloads, app data, app caches, Waydroid);
#    clear app caches (all or per app); remove old system generations;
#    nix-collect-garbage; shrink the journal.
#  * System Settings > Backup: age-encrypted archives (X25519 key wrapped by
#    the user's passphrase) of dconf + chosen ~/.config folders, contacts and
#    history-service databases (sqlite online backup API), optionally
#    photos/videos, documents and Wi-Fi keyfiles; written to a temporary
#    file, verified (full decrypt + SHA-256 of every item), then renamed;
#    preview and selective restore (verified before anything is written);
#    optional daily user timer.
#  * fp6-storaged (Rust, root, system bus com.fairphone6.Storage, D-Bus
#    activated, exits when idle): the only privileged part. Callers: D-Bus
#    policy (phone user only), uid check (exactly the phone user's uid) and
#    polkit com.fairphone6.storage.manage. Generations that are current,
#    running (/run/current-system), booted (/run/booted-system) or pinned by
#    boot_a/boot_b (mobile-nixos.generation= / init= in the boot image
#    command line, read-only) are never deleted; if a boot partition cannot
#    be read, nothing is deleted and no GC runs. Before a GC the kept
#    toplevels become GC roots (/nix/var/nix/gcroots/fp6-boot-pins).
#  * fp6-storage-agent (Rust, user, session bus com.fairphone6.StorageBackup,
#    D-Bus activated): everything else; the Settings pages talk to it through
#    the Fp6.StorageBackup QML module.
#
# Defaults: nothing runs at boot or login; nothing is deleted or backed up
# until the user confirms on a page; daily backup off; no passphrase set.
# Never writes a boot partition.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.storageBackup;
  pkg = pkgs.callPackage ./package.nix { };
  core = pkg.core;
  uid = config.users.users.${cfg.user}.uid;
  home = config.users.users.${cfg.user}.home;

  helperArgs = lib.escapeShellArgs ([
    "--uid" (toString uid)
    "--profiles" "/nix/var/nix/profiles"
    "--profile" "system"
    "--gcroots" "/nix/var/nix/gcroots/fp6-boot-pins"
    "--waydroid-images" "/var/lib/waydroid/images"
    "--wifi-dir" "/etc/NetworkManager/system-connections"
    "--idle-exit" "120"
  ] ++ lib.concatMap (p: [ "--boot-part" "${p}=/dev/disk/by-partlabel/${p}" ]) cfg.bootPartitions);

  dbusSystem = pkgs.writeTextDir "share/dbus-1/system.d/com.fairphone6.Storage.conf" ''
    <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
      <policy user="root">
        <allow own="com.fairphone6.Storage"/>
      </policy>
      <policy user="${cfg.user}">
        <allow send_destination="com.fairphone6.Storage" send_interface="com.fairphone6.Storage1"/>
        <allow send_destination="com.fairphone6.Storage" send_interface="org.freedesktop.DBus.Introspectable"/>
        <allow send_destination="com.fairphone6.Storage" send_interface="org.freedesktop.DBus.Peer"/>
      </policy>
    </busconfig>
  '';
  dbusSystemService = pkgs.writeTextDir "share/dbus-1/system-services/com.fairphone6.Storage.service" ''
    [D-BUS Service]
    Name=com.fairphone6.Storage
    Exec=${pkgs.coreutils}/bin/false
    User=root
    SystemdService=fp6-storaged.service
  '';
  dbusSessionService = pkgs.writeTextDir "share/dbus-1/services/com.fairphone6.StorageBackup.service" ''
    [D-BUS Service]
    Name=com.fairphone6.StorageBackup
    Exec=${pkgs.coreutils}/bin/false
    SystemdService=fp6-storage-agent.service
  '';
  polkitAction = pkgs.writeTextDir "share/polkit-1/actions/com.fairphone6.storage.policy" ''
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
    <policyconfig>
      <action id="com.fairphone6.storage.manage">
        <description>Manage system storage and Wi-Fi backups</description>
        <message>Removing old system versions, unused system files or logs, and backing up Wi-Fi networks requires the phone's user</message>
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
  options.hardware.fairphone6.storageBackup = {
    enable = lib.mkEnableOption "Settings > Storage (storage manager) and Settings > Backup (encrypted backup/restore)";
    user = lib.mkOption {
      type = lib.types.str;
      default = "user";
      description = "The phone account (the only caller the root helper accepts).";
    };
    bootPartitions = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "boot_a" "boot_b" ];
      description = ''
        GPT labels of the boot partitions whose pinned generation is never
        deleted (read-only access). If one of them exists but cannot be read,
        generation removal and garbage collection are refused.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [{
      assertion = config.users.users ? ${cfg.user} && uid != null && uid != 0;
      message = "hardware.fairphone6.storageBackup.user must be an existing non-root account with a fixed uid.";
    }];

    environment.systemPackages = [ core polkitAction ];
    services.dbus.packages = [ dbusSystem dbusSystemService dbusSessionService ];
    security.polkit.enable = true;
    # Lomiri apps (System Settings) run in user@.service outside a logind
    # session, so allow_active never matches (see adb-fp6.nix): grant the
    # phone user explicitly.
    security.polkit.extraConfig = ''
      polkit.addRule(function(action, subject) {
        if (action.id == "com.fairphone6.storage.manage" && subject.user == "${cfg.user}") {
          return polkit.Result.YES;
        }
      });
    '';
    # fp6-backup.timer / .service with an [Install] section, so the agent can
    # `systemctl --user enable --now` it when "Back up every day" is turned on.
    systemd.packages = [ core ];

    systemd.services.fp6-storaged = {
      description = "Storage manager helper (old generations, garbage collection, journal, Wi-Fi backup)";
      # D-Bus activated only; nothing at boot.
      path = [ config.nix.package config.systemd.package pkgs.networkmanager ];
      environment.NIX_REMOTE = "daemon";
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.Storage";
        ExecStart = "${core}/bin/fp6-storaged ${helperArgs}";
        Restart = "no";
        Nice = 10;
        IOSchedulingClass = "idle";
        # Profiles + our GC roots (nix-env --delete-generations), the Nix
        # daemon socket (nix-collect-garbage via the daemon), the journal
        # (vacuum), NetworkManager keyfiles (Wi-Fi restore). Boot partitions
        # are only read (DeviceAllow r).
        ProtectSystem = "strict";
        ReadWritePaths = [
          "/nix/var/nix/profiles" "/nix/var/nix/gcroots" "/nix/var/nix/daemon-socket"
          "/var/log/journal" "-/etc/NetworkManager/system-connections"
        ];
        ProtectHome = "read-only";
        PrivateTmp = true;
        PrivateNetwork = false; # nmcli talks to NetworkManager over D-Bus (AF_UNIX only below)
        RestrictAddressFamilies = [ "AF_UNIX" ];
        DevicePolicy = "closed";
        DeviceAllow = map (p: "/dev/disk/by-partlabel/${p} r") cfg.bootPartitions ++ [ "block-sd r" "block-blkext r" ];
        NoNewPrivileges = true;
        RestrictNamespaces = true;
        RestrictRealtime = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        ProtectClock = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectControlGroups = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" ];
        UMask = "0077";
      };
    };

    systemd.user.services.fp6-storage-agent = {
      description = "Storage and Backup settings agent";
      # D-Bus activated by the Settings pages (and idle after 10 minutes).
      unitConfig.ConditionUser = cfg.user;
      serviceConfig = {
        Type = "dbus";
        BusName = "com.fairphone6.StorageBackup";
        ExecStart = "${core}/bin/fp6-storage-agent serve";
        Nice = 5;
        IOSchedulingClass = "idle";
      };
    };

    # The plugin, and About > Storage opening the new page.
    nixpkgs.overlays = lib.mkOrder 3000 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri-system-settings = (lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ pkg.lssPlugin ];
        })).overrideAttrs (old: {
          postInstall = (old.postInstall or "") + ''
            about=$out/share/lomiri-system-settings/qml-plugins/about/PageComponent.qml
            if [ -e "$about" ]; then
              cp --remove-destination "$(readlink -f "$about")" "$about"
              chmod u+w "$about"
              substituteInPlace "$about" \
                --replace-fail 'Qt.resolvedUrl("Storage.qml")' 'Qt.resolvedUrl("../storage/PageComponent.qml")'
            fi
            test -e $out/share/lomiri-system-settings/storage.settings
            test -e $out/share/lomiri-system-settings/backup.settings
            test -e $out/lib/lomiri-system-settings/private/Fp6/StorageBackup/qmldir
          '';
        });
      });
    }) ];

    system.build.fp6-storage-backup = pkg;
  };
}
