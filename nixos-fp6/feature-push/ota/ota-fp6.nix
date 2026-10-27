# Over-the-air updates with automatic rollback (feature push 2026-10-24).
# Design, slot safety analysis and the supervised phone test:
# results/feature-push-20261024/ota/README.md
#
#  * Release side: `fp6-release publish` copies a toplevel + its boot image
#    into a binary cache signed with the release Nix key and writes a
#    minisign-signed manifest (release counter, store paths, boot image
#    sha256/size, kernel notes, changelog, closure list).
#  * fp6-updated (Rust, root, system bus com.fairphone6.Update, D-Bus
#    activated, exits when idle; callers: the phone user via uid check +
#    polkit com.fairphone6.update.manage, and root): check (manifest
#    signature with the baked-in key), download (nix copy --from the cache,
#    store signatures enforced by extra-trusted-public-keys; Wi-Fi/charging/
#    battery saver policy; resumable per store path), install, rollback.
#  * Install, hardware.fairphone6.ota.allowSlotSwitch = false (DEFAULT):
#    today's single-slot procedure, automated: back up boot_<running>
#    (whole partition + trimmed image), write it (no truncation), fsync,
#    readback sha256, set the system profile. No automatic rollback; the
#    page says so. Rollback/undo writes the backup back.
#  * Install, allowSlotSwitch = true (only after the supervised slot test):
#    every *_a/*_b pair except boot must be byte-identical (else refused),
#    the image goes to the INACTIVE boot partition, then qbootctl -s,
#    attributes verified; fp6-update-health.timer (once per boot) runs the
#    start check, marks the slot successful (qbootctl -m) or re-arms / rolls
#    back and reboots. The always-on qbootctl.service (-m at every boot) is
#    disabled in this mode so a bad boot is never marked successful.
#  * Settings > System update; About shows the release (ro.build.display.id).
#
# Writes happen ONLY in install/rollback (confirmed on the page, or "install
# tonight" chosen there) and in the health check of a pending A/B update.
# Never touches userdata, persist, modemst*/fsg/fsc or any non-boot partition.
{ config, lib, pkgs, options, ... }:
let
  cfg = config.hardware.fairphone6.ota;
  pkg = pkgs.callPackage ./package.nix { };
  core = pkg.core;
  rel = import ./release.nix;
  keys = import ./trusted-keys.nix;
  uid = config.users.users.${cfg.user}.uid;
  kernelRelease = config.boot.kernelPackages.kernel.modDirVersion or "";

  releaseInfo = {
    device = cfg.device;
    channel = cfg.channel;
    release = cfg.release;
    version = cfg.version;
    label = config.system.nixos.label;
    kernel = kernelRelease;
  };

  daemonConfig = {
    inherit uid;
    polkitAction = "com.fairphone6.update.manage";
    device = cfg.device;
    channel = cfg.channel;
    server = cfg.server;
    cacheUrl = cfg.cacheUrl;
    manifestPublicKey = cfg.manifestPublicKey;
    nixPublicKey = cfg.nixPublicKey;
    allowSlotSwitch = cfg.allowSlotSwitch;
    maxBootAttempts = cfg.maxBootAttempts;
    allowedSlotDifferences = cfg.allowedSlotDifferences;
    bootPartitionSize = cfg.bootPartitionSize;
    nix = "${config.nix.package}/bin/nix";
    nixEnv = "${config.nix.package}/bin/nix-env";
    qbootctl = "${pkgs.qbootctl}/bin/qbootctl";
    systemctl = "${config.systemd.package}/bin/systemctl";
    criticalUnits = cfg.healthCheck.criticalUnits;
    netbirdUnit = if config.services.netbird.clients ? netbird then "netbird.service" else null;
    healthTimeout = cfg.healthCheck.timeout;
    healthStable = cfg.healthCheck.stableSeconds;
  };

  dbusSystem = pkgs.writeTextDir "share/dbus-1/system.d/com.fairphone6.Update.conf" ''
    <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
    <busconfig>
      <policy user="root">
        <allow own="com.fairphone6.Update"/>
        <allow send_destination="com.fairphone6.Update" send_interface="com.fairphone6.Update1"/>
      </policy>
      <policy user="${cfg.user}">
        <allow send_destination="com.fairphone6.Update" send_interface="com.fairphone6.Update1"/>
        <allow send_destination="com.fairphone6.Update" send_interface="org.freedesktop.DBus.Introspectable"/>
        <allow send_destination="com.fairphone6.Update" send_interface="org.freedesktop.DBus.Peer"/>
      </policy>
    </busconfig>
  '';
  dbusSystemService = pkgs.writeTextDir "share/dbus-1/system-services/com.fairphone6.Update.service" ''
    [D-BUS Service]
    Name=com.fairphone6.Update
    Exec=${pkgs.coreutils}/bin/false
    User=root
    SystemdService=fp6-updated.service
  '';
  polkitAction = pkgs.writeTextDir "share/polkit-1/actions/com.fairphone6.update.policy" ''
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
     "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
    <policyconfig>
      <action id="com.fairphone6.update.manage">
        <description>Check for, download and install system updates</description>
        <message>Installing system updates requires the phone's user</message>
        <defaults>
          <allow_any>no</allow_any>
          <allow_inactive>no</allow_inactive>
          <allow_active>yes</allow_active>
        </defaults>
      </action>
    </policyconfig>
  '';

  bootParts = [ "/dev/disk/by-partlabel/boot_a" "/dev/disk/by-partlabel/boot_b" ];
  hardening = {
    ProtectSystem = "strict";
    ProtectHome = "read-only";
    PrivateTmp = true;
    NoNewPrivileges = true;
    RestrictNamespaces = true;
    RestrictRealtime = true;
    LockPersonality = true;
    ProtectClock = true;
    ProtectHostname = true;
    ProtectKernelLogs = true;
    ProtectKernelModules = true;
    ProtectKernelTunables = true;
    ProtectControlGroups = true;
    SystemCallArchitectures = "native";
    SystemCallFilter = [ "@system-service" ];
    UMask = "0077";
    # Boot partitions read-write, everything else on UFS read-only (pair
    # hashes, GPT reads); with slot switching qbootctl needs the whole LUNs
    # (GPT) and the UFS BSG node (boot LUN) read-write.
    DevicePolicy = "closed";
    DeviceAllow = (map (p: "${p} rw") bootParts)
      ++ (if cfg.allowSlotSwitch then [ "block-sd rw" "char-bsg rw" ] else [ "block-sd r" ]);
  };
in
{
  options.hardware.fairphone6.ota = {
    enable = lib.mkEnableOption "signed OTA updates (Settings > System update) with optional A/B rollback";
    user = lib.mkOption {
      type = lib.types.str;
      default = "user";
      description = "The phone account (the only non-root caller fp6-updated accepts).";
    };
    device = lib.mkOption { type = lib.types.str; default = "fairphone-fp6"; };
    channel = lib.mkOption {
      type = lib.types.str;
      default = rel.channel;
      description = "Update channel (manifest at <server>/channels/<channel>/manifest.json).";
    };
    release = lib.mkOption {
      type = lib.types.ints.positive;
      default = rel.release;
      description = "Release counter baked into this system (ota/release.nix; bump per published update).";
    };
    version = lib.mkOption {
      type = lib.types.str;
      readOnly = true;
      default = "FP6 r${toString cfg.release} (${config.system.nixos.label})";
      description = "Version string shown in About and on the System update page.";
    };
    server = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "http://<NetBird address of the release machine>:8742";
      description = ''
        Update server root (https://, or http:// over NetBird 100.x / loopback;
        manifests and store paths are signed either way). null: none
        configured; the coordinator can set one at runtime with
        `fp6-updated call setServer '{"server":"..."}'` as root.
      '';
    };
    cacheUrl = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Binary cache override (else the manifest's cache, relative to the manifest URL).";
    };
    manifestPublicKey = lib.mkOption {
      type = lib.types.str;
      default = keys.manifest;
      description = "minisign public key (base64 line) for manifest.json.minisig.";
    };
    nixPublicKey = lib.mkOption {
      type = lib.types.str;
      default = keys.nix;
      description = "Nix signing key every store path of an update must carry (added to extra-trusted-public-keys).";
    };
    allowSlotSwitch = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        A/B installs (inactive slot, qbootctl -s, start check, automatic
        rollback). OFF until the supervised slot test in the README passed
        and the coordinator and the user agreed. Off: single-slot installs
        to the running slot's boot partition, with a backup, no automatic
        rollback.
      '';
    };
    maxBootAttempts = lib.mkOption {
      type = lib.types.ints.between 1 7;
      default = 2;
      description = "Boots of a new slot that may fail the start check before the daemon switches back.";
    };
    allowedSlotDifferences = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "A/B pair names allowed to differ between slots (only after the README's analysis).";
    };
    bootPartitionSize = lib.mkOption {
      type = lib.types.ints.positive;
      default = 100663296;
      description = "Expected size of boot_a/boot_b; anything else is refused.";
    };
    healthCheck = {
      timeout = lib.mkOption { type = lib.types.ints.positive; default = 300; description = "Seconds to wait for the session."; };
      stableSeconds = lib.mkOption { type = lib.types.ints.positive; default = 30; description = "The shell must survive this long."; };
      criticalUnits = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ "display-manager.service" "dbus.service" "systemd-logind.service" "NetworkManager.service" "sshd.service" "ofono.service" "rmtfs.service" ];
        description = "Units that must not be failed.";
      };
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      assertions = [
        { assertion = config.users.users ? ${cfg.user} && uid != null && uid != 0;
          message = "hardware.fairphone6.ota.user must be an existing non-root account with a fixed uid."; }
        { assertion = cfg.manifestPublicKey != "" && cfg.nixPublicKey != "";
          message = "hardware.fairphone6.ota needs the release public keys (ota/trusted-keys.nix)."; }
      ];

      environment.etc."fp6-release".text = builtins.toJSON releaseInfo;
      environment.etc."fp6-updated/config.json".text = builtins.toJSON daemonConfig;
      environment.systemPackages = [ core pkgs.qbootctl polkitAction ];

      nix.settings.extra-trusted-public-keys = [ cfg.nixPublicKey ];

      services.dbus.packages = [ dbusSystem dbusSystemService ];
      security.polkit.enable = true;
      # System Settings runs outside a logind session (see storage-backup):
      # grant the phone user explicitly.
      security.polkit.extraConfig = ''
        polkit.addRule(function(action, subject) {
          if (action.id == "com.fairphone6.update.manage" && subject.user == "${cfg.user}") {
            return polkit.Result.YES;
          }
        });
      '';

      systemd.services.fp6-updated = {
        description = "System update service (check, download, install, rollback)";
        # D-Bus activated only; nothing at boot.
        path = [ config.nix.package pkgs.curl pkgs.qbootctl config.systemd.package pkgs.coreutils ];
        environment = {
          NIX_REMOTE = "daemon";
          XDG_CACHE_HOME = "/var/lib/fp6-updated/cache";
          SSL_CERT_FILE = "/etc/ssl/certs/ca-certificates.crt";
        };
        serviceConfig = hardening // {
          Type = "dbus";
          BusName = "com.fairphone6.Update";
          ExecStart = "${core}/bin/fp6-updated serve";
          Restart = "no";
          Nice = 10;
          IOSchedulingClass = "idle";
          StateDirectory = "fp6-updated";
          StateDirectoryMode = "0700";
          ReadWritePaths = [ "/nix/var/nix/profiles" "/nix/var/nix/gcroots" "/nix/var/nix/daemon-socket" ];
          RestrictAddressFamilies = [ "AF_UNIX" "AF_INET" "AF_INET6" "AF_NETLINK" ];
          # A running install is never interrupted by the idle exit; give a
          # stop during one time to cancel cleanly.
          TimeoutStopSec = 60;
        };
      };

      # Automatic check/download and "install tonight": every 30 minutes the
      # service decides (policy, interval, night window); it exits when idle.
      systemd.services.fp6-updated-scheduled = {
        description = "System update: scheduled check, download, night install";
        serviceConfig = {
          Type = "oneshot";
          ExecStart = "${core}/bin/fp6-updated call scheduled";
        };
      };
      systemd.timers.fp6-updated-scheduled = {
        wantedBy = [ "timers.target" ];
        timerConfig = { OnBootSec = "15min"; OnCalendar = "*:0/30"; RandomizedDelaySec = "5min"; };
      };

      # Start check, once per boot (a timer, so no target waits for it).
      systemd.services.fp6-update-health = {
        description = "System update: start check of this boot";
        after = [ "display-manager.service" "network.target" "sshd.service" ];
        path = [ pkgs.qbootctl config.systemd.package pkgs.coreutils ];
        serviceConfig = hardening // {
          Type = "oneshot";
          ExecStart = "${core}/bin/fp6-updated health-check";
          StateDirectory = "fp6-updated";
          StateDirectoryMode = "0700";
          RestrictAddressFamilies = [ "AF_UNIX" "AF_NETLINK" ];
          TimeoutStartSec = cfg.healthCheck.timeout + cfg.healthCheck.stableSeconds + 120;
        };
      };
      systemd.timers.fp6-update-health = {
        wantedBy = [ "timers.target" ];
        timerConfig = { OnBootSec = "45s"; AccuracySec = "5s"; };
      };

      # The existing unit marks the running slot successful at every boot.
      # With slot switching that would confirm a broken update; the start
      # check does it instead.
      systemd.services.qbootctl.enable = lib.mkIf cfg.allowSlotSwitch (lib.mkForce false);

      # The page.
      nixpkgs.overlays = lib.mkOrder 3000 [ (_final: prev: {
        lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
          lomiri-system-settings = (lprev.lomiri-system-settings.override (old: {
            plugins = (old.plugins or [ ]) ++ [ pkg.lssPlugin ];
          })).overrideAttrs (old: {
            postInstall = (old.postInstall or "") + ''
              test -e $out/share/lomiri-system-settings/update.settings
              test -e $out/lib/lomiri-system-settings/private/Fp6/Update/qmldir
            '';
          });
        });
      }) ];

      system.build.fp6-ota = pkg;
      system.build.fp6-release-info = pkgs.writeText "fp6-release.json" (builtins.toJSON releaseInfo);
    }
    # About > OS shows the release (the settings module's property shim).
    (lib.optionalAttrs (options.hardware.fairphone6 ? settingsPanels) {
      hardware.fairphone6.settingsPanels.properties =
        options.hardware.fairphone6.settingsPanels.properties.default // {
          "ro.build.display.id" = "NixOS ${cfg.version}";
        };
    })
  ]);
}
