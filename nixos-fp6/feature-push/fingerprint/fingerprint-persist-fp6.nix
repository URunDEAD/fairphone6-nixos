# FP6 fingerprint with WRITABLE persistent secure storage: the postmarketOS
# layout, on NixOS. Supersedes the read-only/private-root modes of
# ../../fingerprint-preserved-fp6.nix for the actual enrollment/unlock path.
#
# What the trustlet (focal64, via QTEE) writes, and how (identical to pmOS
# fingerprintd 0.2.6, packaging/{fingerprintd.service,mnt-persist.mount,
# fingerprintd.tmpfiles.conf}, byte-identical to the files on /run/nura):
#   * GPFile listener 0x7000 (served by fingerprintd itself): SFS containers
#     under the SFS root. Roots: tzstorage, misc (pmOS rootfs dirs) and
#     persist-data == root3 -> /mnt/persist/data (Android persist, sda6).
#   * RPMB listener 0x2000 (served by fingerprintd itself): authenticated
#     RPMB read/write frames relayed to the UFS RPMB W-LU /dev/bsg/0:0:0:49476.
#     Key-programming frames (req 0x0001) are refused unconditionally by
#     upstream (Fingerprintd-Rpmb.cppm AnyKeyProgramming, checked on every
#     frame before any write policy); this module adds nothing that can send
#     one, and no step here issues RPMB commands of its own.
# There is no qseecomd/tee-supplicant: fingerprintd is the only listener.
#
# The SFS root and map are the pmOS ones on /run/nura so both OSes share ONE
# secure-storage state (the RPMB anti-rollback counter is device-global;
# a second, diverging copy would be rejected by whichever OS is older).
#
# Nothing starts at boot unless startAtBoot. The first start of the boot runs
# fp6-fingerprint-persist-backup.service, which images the (unmounted)
# persist partition and tars the pmOS SFS tree + map into
# <backupRoot>/fingerprint-backup-<UTC>/ with SHA256SUMS before
# mnt-persist.mount mounts it read-write. No raw partition is ever written;
# persist is written only as ext4 (journal/superblock on mount, exactly as
# pmOS did on every boot) and by the trustlet through the GPFile listener.
{ config, lib, pkgs, utils, ... }:
let
  cfg = config.services.fp6FingerprintPersist;
  # Fixed: the pmOS SFS symlinks persist-data/root3 point at /mnt/persist/data.
  mountPoint = "/mnt/persist";
  device = "/dev/disk/by-uuid/${cfg.persistUuid}";
  sfsRoot = "${cfg.stateDir}/sfs";
  actions = pkgs.writeText "fp6-fingerprint-persist-no-actions.conf" "";
  path = with pkgs; [ coreutils util-linux e2fsprogs gnutar findutils systemd gawk gnugrep ];

  args = [
    "${cfg.package}/bin/fingerprintd" "--daemon" "--edge-wake"
    "--ta=${cfg.trustlet}" "--config=${cfg.configFile}"
    "--state-dir=${cfg.stateDir}" "--sfs-root=${sfsRoot}"
    # pmOS: "--sfs-writable --rpmb-write" globally (init, SET_ACTIVE_GROUP and
    # SAVE). The preserved package's transaction-only policy refused 3 RPMB
    # writes in INIT_DEVICE and 12 in each SET_ACTIVE_GROUP before SAVE_DATA
    # returned -EIO (results/fingerprint-touch-review/phone-enrollment02-daemon.log).
    "--sfs-writable" "--rpmb-write"
    "--actions=${actions}" "--learn=0"
    "--standby-unlock=${if cfg.standbyUnlock then "on" else "off"}"
  ];

  otherOwners = [ "fp6-fingerprint-preserved.service" "fp6-fingerprintd.service"
                  "fp6-fingerprint-readonly.service" "fingerprintd.service" "fprintd.service" ];

  refuseOtherOwners = ''
    for unit in ${lib.concatStringsSep " " otherOwners}; do
      if systemctl is-active --quiet "$unit"; then
        echo "refusing: $unit already owns the sensor/QTEE listeners" >&2; exit 1
      fi
    done
  '';

  backup = pkgs.writeShellScript "fp6-fingerprint-persist-backup" ''
    set -euo pipefail
    export PATH=${lib.makeBinPath path}
    umask 077
    ${refuseOtherOwners}
    dev=$(readlink -f ${device})
    test -b "$dev"
    test "$(blkid -s PARTLABEL -o value "$dev")" = persist
    test "$(blkid -s TYPE -o value "$dev")" = ext4
    # The image must be of a quiescent filesystem: refuse while mounted anywhere
    # (also catches the old read-only /run/fp6-fingerprint-persist mount).
    if findmnt -rn -S "$dev" >/dev/null; then
      echo "refusing: $dev is mounted; unmount it before the first writable start" >&2; exit 1
    fi
    test -d ${lib.escapeShellArg cfg.stateDir}
    ts=$(date -u +%Y%m%dT%H%M%SZ)
    dir=${lib.escapeShellArg cfg.backupRoot}/fingerprint-backup-$ts
    need=$(( $(blockdev --getsize64 "$dev") / 1024 + 65536 ))
    free=$(df -Pk ${lib.escapeShellArg cfg.backupRoot} | awk 'NR==2{print $4}')
    test "$free" -gt "$need"
    install -d -m 0700 -o root -g root "$dir"
    # Read-only filesystem check (-n: opens the device read-only, answers no).
    if ! e2fsck -fn "$dev" >"$dir/e2fsck-n.txt" 2>&1; then
      echo "refusing: e2fsck -fn reported problems on persist (see $dir/e2fsck-n.txt)" >&2; exit 1
    fi
    dumpe2fs -h "$dev" >"$dir/persist-superblock.txt" 2>/dev/null
    dd if="$dev" of="$dir/persist.img" bs=1M status=none
    sync -f "$dir/persist.img"
    devsum=$(sha256sum <"$dev" | cut -d' ' -f1)
    imgsum=$(sha256sum <"$dir/persist.img" | cut -d' ' -f1)
    if [ "$devsum" != "$imgsum" ]; then
      echo "refusing: persist image hash differs from the device" >&2; exit 1
    fi
    tar --xattrs --acls --numeric-owner -cpf "$dir/pmos-fingerprintd-state.tar" \
      -C ${lib.escapeShellArg (builtins.dirOf cfg.stateDir)} ${lib.escapeShellArg (builtins.baseNameOf cfg.stateDir)}
    {
      echo "device $dev partuuid $(blkid -s PARTUUID -o value "$dev") uuid ${cfg.persistUuid}"
      echo "device-sha256 $devsum"
      echo "trustlet-sha256 $(sha256sum <${lib.escapeShellArg cfg.trustlet} | cut -d' ' -f1)"
      echo "boot-id $(cat /proc/sys/kernel/random/boot_id) kernel $(uname -r)"
    } >"$dir/MANIFEST"
    (cd "$dir" && sha256sum persist.img pmos-fingerprintd-state.tar persist-superblock.txt e2fsck-n.txt MANIFEST >SHA256SUMS)
    sync -f "$dir/SHA256SUMS"
    ln -sfn "$dir" /run/fp6-fingerprint-persist-backup.last
    echo "persist backup complete: $dir"
  '';

  prerequisites = pkgs.writeShellScript "fp6-fingerprint-persist-prerequisites" ''
    set -euo pipefail
    export PATH=${lib.makeBinPath path}
    ${refuseOtherOwners}
    # At boot the QTEE and UFS RPMB nodes can appear after this unit is
    # scheduled (no systemd device units for them): wait up to 60 s, then fail.
    for i in $(seq 1 120); do
      test -c /dev/tee0 && test -c /dev/bsg/0:0:0:49476 && break
      sleep 0.5
    done
    test -c /dev/tee0
    test -c /dev/bsg/0:0:0:49476
    test "$(sha256sum <${lib.escapeShellArg cfg.trustlet} | cut -d' ' -f1)" = ${cfg.trustletSha256}
    # Backup of this boot must exist and verify.
    last=$(readlink -f /run/fp6-fingerprint-persist-backup.last)
    (cd "$last" && sha256sum --quiet -c SHA256SUMS)
    # persist must be the verified device, mounted read-write at ${mountPoint}.
    test "$(readlink -f "$(findmnt -rn -o SOURCE ${mountPoint})")" = "$(readlink -f ${device})"
    findmnt -rn -o OPTIONS ${mountPoint} | grep -Eq '(^|,)rw(,|$)'
    test -d ${mountPoint}/data
    # Exact pmOS SFS layout (tmpfiles.d/fingerprintd.conf); nothing is repaired.
    for d in ${cfg.stateDir} ${sfsRoot} ${sfsRoot}/tzstorage ${sfsRoot}/misc; do
      test -d "$d"; test ! -L "$d"
      test "$(stat -c %u:%a "$d")" = 0:700
    done
    for l in persist-data root3; do
      test "$(readlink ${sfsRoot}/$l)" = ${mountPoint}/data
    done
  '';

  clients = pkgs.runCommand "fp6-fprintd-clients-${pkgs.fprintd.version}" {} ''
    # Only the D-Bus clients; no fprintd daemon, unit or bus activation file.
    mkdir -p $out/bin
    for b in fprintd-enroll fprintd-verify fprintd-list fprintd-delete; do
      ln -s ${pkgs.fprintd}/bin/$b $out/bin/$b
    done
  '';

  bridgePython = pkgs.python3.withPackages (p: [ p.dbus-python p.pygobject3 ]);
  bridge = pkgs.runCommand "fp6-lomiri-fingerprint-lockedhint-1" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
  } ''
    install -Dm644 ${./fp6-lomiri-fingerprint-lockedhint.py} $out/lib/fp6_lockedhint.py
    ${bridgePython}/bin/python3 -m py_compile $out/lib/fp6_lockedhint.py
    makeWrapper ${bridgePython}/bin/python3 $out/bin/fp6-lomiri-fingerprint-lockedhint \
      --add-flags "-W ignore::DeprecationWarning $out/lib/fp6_lockedhint.py"
  '';
in {
  options.services.fp6FingerprintPersist = {
    enable = lib.mkEnableOption "the FP6 fingerprint daemon with writable persistent secure storage (pmOS layout)";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ../../fingerprintd-transport-package.nix {};
      description = "Upstream fingerprintd a4d5ed6 (= pmOS 0.2.6) plus errno-only transport diagnostics; no storage-policy changes.";
    };
    trustlet = lib.mkOption {
      type = lib.types.str;
      default = "/run/nura/usr/lib/firmware/focal64.mbn";
      description = "Device-signed OEM trustlet, used in place (never copied).";
    };
    trustletSha256 = lib.mkOption {
      type = lib.types.str;
      default = "9efded87abd3c09a7d62529c64cf5605d169a027766e1240249d59a518fbc88d";
      description = "Expected trustlet hash (the pmOS file read on 2026-10-24).";
    };
    configFile = lib.mkOption {
      type = lib.types.str;
      default = "/run/nura/usr/lib/firmware/fingerprintd.json";
      description = "pmOS fingerprintd.json (identical to upstream packaging/fingerprintd.json).";
    };
    stateDir = lib.mkOption {
      type = lib.types.str;
      default = "/run/nura/var/lib/fingerprintd";
      description = "pmOS state dir: fingers-<uid>.map plus sfs/{tzstorage,misc,persist-data,root3}.";
    };
    persistUuid = lib.mkOption {
      type = lib.types.str;
      default = "b1444981-73a3-404c-90a3-89f898340fbd";
      description = "ext4 UUID of the verified persist partition (sda6, PARTLABEL persist).";
    };
    backupRoot = lib.mkOption {
      type = lib.types.str;
      default = "/run/nura/home/user/fp6-install";
      description = "Directory receiving fingerprint-backup-<UTC>/ before the first writable mount of each boot.";
    };
    startAtBoot = lib.mkEnableOption "starting the writable daemon at boot (only after a verified live trial)";
    standbyUnlock = lib.mkEnableOption "fingerprintd's logind standby unlock (no effect until a finger is enrolled)";
    lomiriLockedHint = lib.mkEnableOption "the Lomiri greeter -> logind LockedHint bridge that lets standby unlock arm on Lomiri";
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !config.services.fprintd.enable
          && !(config.services.fp6Fingerprint.enable or false)
          && !(config.services.fp6FingerprintReadOnly.enable or false)
          && !((config.services.fp6FingerprintPreserved.enable or false)
               && (config.services.fp6FingerprintPreserved.startAtBoot or false));
        message = "fp6FingerprintPersist must be the only fingerprint daemon that can start (others disabled or manual-only).";
      }
      {
        assertion = lib.all (p: lib.hasPrefix "/" p) [ cfg.trustlet cfg.configFile cfg.stateDir cfg.backupRoot ];
        message = "fp6FingerprintPersist paths must be absolute.";
      }
      {
        assertion = !cfg.lomiriLockedHint || cfg.standbyUnlock;
        message = "The Lomiri LockedHint bridge is only useful with standbyUnlock.";
      }
    ];

    environment.systemPackages = [ clients ];
    services.dbus.packages = [ cfg.package ];
    system.build.fp6-fingerprint-persist = cfg.package;
    system.build.fp6-lomiri-fingerprint-lockedhint = bridge;

    systemd.services.fp6-fingerprint-persist-backup = {
      description = "FP6: back up Android persist + pmOS SFS tree before writable fingerprint storage";
      unitConfig = {
        # Assert (fail), never Condition (skip = success, which would let the
        # writable mount proceed without a backup).
        AssertPathExists = [ cfg.stateDir cfg.backupRoot ];
        RequiresMountsFor = [ cfg.backupRoot cfg.stateDir ];
        # mnt-persist.mount is ordered after this unit and was part of
        # local-fs.target (before nofail); default dependencies (after
        # basic.target -> sysinit.target -> local-fs.target) closed a cycle and
        # systemd dropped systemd-tmpfiles-setup and others to break it
        # (2026-10-26). Kept off the boot targets either way.
        DefaultDependencies = false;
      };
      conflicts = [ "shutdown.target" ];
      before = [ "mnt-persist.mount" "shutdown.target" ];
      # The persist by-uuid link is a udev device unit: wait for it at boot
      # (a path Assert alone could run before udev and fail the chain).
      requires = [ "${utils.escapeSystemdPath device}.device" ];
      after = [ "${utils.escapeSystemdPath device}.device" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;   # once per boot
        ExecStart = backup;
        UMask = "0077";
      };
    };

    systemd.mounts = [ {
      description = "Android persist partition (writable, for QTEE SFS)";
      what = device;
      where = mountPoint;
      type = "ext4";
      # pmOS mnt-persist.mount options, plus nofail: a local mount unit is
      # implicitly Before=local-fs.target unless it has nofail, so the boot
      # (local-fs -> sysinit -> basic -> display-manager) waited for the
      # persist device and the ~1 s backup. With nofail only
      # fp6-fingerprint-persist (Requires/After below) waits for this mount.
      options = "rw,nosuid,nodev,noexec,nofail";
      requires = [ "fp6-fingerprint-persist-backup.service" ];
      after = [ "fp6-fingerprint-persist-backup.service" ];
      unitConfig.ConditionPathExists = device;
      # Deliberately no wantedBy: only fp6-fingerprint-persist pulls it in.
    } ];

    # x-systemd.device-timeout only works in fstab (systemd.mount(5)); this
    # is what fstab-generator writes for it. Without the device, the backup,
    # the mount and the daemon fail after 30 s instead of the default 90 s.
    # Nothing in the boot targets waits for this device job.
    systemd.units."${utils.escapeSystemdPath device}.device" = {
      overrideStrategy = "asDropin";
      text = ''
        [Unit]
        JobRunningTimeoutSec=30s
      '';
    };

    systemd.services.fp6-fingerprint-persist = {
      description = "FP6 fingerprint daemon, writable persistent secure storage (pmOS layout)";
      wantedBy = lib.optional cfg.startAtBoot "multi-user.target";
      requires = [ "dbus.service" "mnt-persist.mount" ];
      after = [ "dbus.service" "systemd-modules-load.service" "mnt-persist.mount" ];
      conflicts = otherOwners;
      unitConfig = {
        # Device nodes are waited for in ExecStartPre (a Condition would
        # silently skip the daemon if udev had not created them yet).
        ConditionPathExists = [ cfg.trustlet ];
        RequiresMountsFor = [ mountPoint cfg.stateDir ];
      };
      serviceConfig = {
        Type = "simple";
        ExecStartPre = prerequisites;
        ExecStart = lib.escapeShellArgs args;
        UMask = "0077";
        # pmOS declares no sandboxing (root drives gpiochip, /dev/tee0 and the
        # raw RPMB W-LU); kept identical to what pmOS ran.
        Restart = if cfg.startAtBoot then "on-failure" else "no";
        RestartSec = 5;
        KillMode = "mixed";
        TimeoutStopSec = 15;
      };
    };

    systemd.user.services.fp6-lomiri-fingerprint-lockedhint = lib.mkIf cfg.lomiriLockedHint {
      description = "FP6: mirror the Lomiri lock screen into logind LockedHint for fingerprint standby unlock";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      serviceConfig = {
        ExecStart = "${bridge}/bin/fp6-lomiri-fingerprint-lockedhint";
        Restart = "on-failure";
        RestartSec = 5;
      };
    };
  };
}
