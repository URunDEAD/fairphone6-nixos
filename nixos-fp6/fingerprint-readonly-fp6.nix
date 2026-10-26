# Optional bounded sensor-initialization service. Never mounts Android persist,
# enrolls a finger, enables PAM, or permits SFS/RPMB writes.
{ config, lib, pkgs, ... }:
let
  cfg = config.services.fp6FingerprintReadOnly;
  state = "/var/lib/fp6-fingerprint-readonly";
  disabledActions = pkgs.writeText "fp6-fingerprint-readonly-no-actions.conf" "";
  beforeStart = pkgs.writeShellScript "fp6-fingerprint-readonly-prerequisites" ''
    set -eu
    for unit in fingerprintd.service fp6-fingerprintd.service fprintd.service; do
      if ${pkgs.systemd}/bin/systemctl is-active --quiet "$unit"; then
        echo "Refusing a second fingerprint owner: $unit is active" >&2
        exit 1
      fi
    done
    test -c /dev/tee0
    test -c /dev/bsg/0:0:0:49476
  '';
in {
  options.services.fp6FingerprintReadOnly = {
    enable = lib.mkEnableOption "an explicit, bounded FP6 QTEE sensor initialization with original storage read-only";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./fingerprintd-package.nix {};
    };
    trustlet = lib.mkOption {
      type = lib.types.str;
      default = "/run/nura/usr/lib/firmware/focal64.mbn";
      description = "Existing OEM trustlet runtime filename; never copied into the Nix store.";
    };
    persistMountPoint = lib.mkOption {
      type = lib.types.str;
      default = "/run/fp6-fingerprint-persist";
      description = "Already mounted Android persist; this module creates no mount unit.";
    };
    legacySfsRoot = lib.mkOption {
      type = lib.types.str;
      default = "/run/nura/var/lib/fingerprintd/sfs";
      description = "Existing pmOS SFS root; only tzstorage/misc are bound read-only, never copied.";
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [ {
      assertion = !config.services.fprintd.enable
        && !(config.services.fp6Fingerprint.enable or false);
      message = "The bounded FP6 sensor initialization requires other fingerprint daemon modules disabled.";
    } ];
    # The package supplies bus policy only, with no D-Bus activation file.
    environment.systemPackages = [ cfg.package ];
    services.dbus.packages = [ cfg.package ];
    systemd.tmpfiles.rules = map
      (suffix: "d ${state}${suffix} 0700 root root -")
      [ "" "/sfs" "/sfs/tzstorage" "/sfs/misc" "/sfs/persist-data" "/sfs/root3" ];
    systemd.services.fp6-fingerprint-readonly = {
      description = "Bounded FP6 fingerprint sensor initialization; SFS/RPMB writes disabled";
      wantedBy = [];
      requires = [ "dbus.service" ];
      after = [ "dbus.service" "systemd-modules-load.service" ];
      unitConfig = {
        ConditionPathExists = [ cfg.trustlet "/dev/tee0" "/dev/bsg/0:0:0:49476" ];
        ConditionPathIsMountPoint = cfg.persistMountPoint;
        ConditionPathIsDirectory = [
          "${cfg.persistMountPoint}/data"
          "${cfg.legacySfsRoot}/tzstorage"
          "${cfg.legacySfsRoot}/misc"
        ];
      };
      serviceConfig = {
        Type = "simple";
        ExecStartPre = beforeStart;
        ExecStart = lib.escapeShellArgs [
          "${cfg.package}/bin/fingerprintd" "--daemon" "--edge-wake"
          "--ta=${cfg.trustlet}"
          "--config=${cfg.package}/share/fingerprintd/fingerprintd.json"
          "--state-dir=${state}" "--sfs-root=${state}/sfs"
          "--actions=${disabledActions}" "--learn=0" "--standby-unlock=off"
        ];
        StateDirectory = "fp6-fingerprint-readonly";
        StateDirectoryMode = "0700";
        ProtectSystem = "strict";
        ReadOnlyPaths = [ cfg.persistMountPoint cfg.legacySfsRoot ];
        BindReadOnlyPaths = [
          "${cfg.persistMountPoint}/data:${state}/sfs/persist-data"
          "${cfg.persistMountPoint}/data:${state}/sfs/root3"
          "${cfg.legacySfsRoot}/tzstorage:${state}/sfs/tzstorage"
          "${cfg.legacySfsRoot}/misc:${state}/sfs/misc"
        ];
        Restart = "no";
        RuntimeMaxSec = 90;
        KillMode = "mixed";
        TimeoutStopSec = 15;
      };
    };
  };
}
