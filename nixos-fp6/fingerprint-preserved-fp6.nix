# Opt-in integration only. Original Android/pmOS fingerprint storage remains
# read-only; new templates belong to a separate root-only NixOS store.
{ config, lib, pkgs, ... }:
let
  cfg = config.services.fp6FingerprintPreserved;
  state = "/var/lib/fp6-fingerprint-preserved";
  enrollment = cfg.mode == "enrollment";
  actions = pkgs.writeText "fp6-fingerprint-preserved-no-actions.conf" "";
  args = [
    "${cfg.package}/bin/fingerprintd" "--daemon" "--edge-wake"
    "--ta=${cfg.trustlet}"
    "--config=${cfg.package}/share/fingerprintd/fingerprintd.json"
    "--state-dir=${state}" "--sfs-root=${state}/sfs"
    "--actions=${actions}" "--learn=0"
    "--standby-unlock=${if cfg.standbyUnlock then "on" else "off"}"
  ] ++ lib.optionals enrollment [
    "--sfs-writable" "--rpmb-write" "--allow-empty-user-group=10000"
  ] ++ lib.optional cfg.allowExplicitRemoval "--allow-template-removal";
  prerequisites = pkgs.writeShellScript "fp6-fingerprint-preserved-prerequisites" ''
    set -eu
    for unit in fingerprintd.service fp6-fingerprintd.service fp6-fingerprint-readonly.service fprintd.service; do
      if ${pkgs.systemd}/bin/systemctl is-active --quiet "$unit"; then
        echo "Refusing a second fingerprint owner: $unit is active" >&2
        exit 1
      fi
    done
    test -c /dev/tee0
    test -c /dev/bsg/0:0:0:49476
    test -r ${lib.escapeShellArg cfg.trustlet}
    # Refuse substituted roots. Nothing here copies or changes OEM storage.
    for path in ${state} ${state}/sfs ${state}/sfs/persist-data ${state}/sfs/root3; do
      test -d "$path"
      test ! -L "$path"
      test "$(${pkgs.coreutils}/bin/stat -c %u "$path")" = 0
      test "$(${pkgs.coreutils}/bin/stat -c %a "$path")" = 700
    done
  '';
in {
  options.services.fp6FingerprintPreserved = {
    enable = lib.mkEnableOption "the preservation-guarded FP6 fingerprint daemon";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./fingerprintd-preserved-package.nix {};
      description = "Pinned daemon with transport diagnostics and enrollment-preservation guards.";
    };
    mode = lib.mkOption {
      type = lib.types.enum [ "read-only" "enrollment" ];
      default = "read-only";
      description = "Read-only disables trustlet storage writes. Enrollment permits writes only during explicit enrollment (or separately enabled explicit removal).";
    };
    trustlet = lib.mkOption {
      type = lib.types.str;
      default = "/run/nura/usr/lib/firmware/focal64.mbn";
      description = "Existing device-signed OEM trustlet runtime path; never copied into the Nix store.";
    };
    legacySfsRoot = lib.mkOption {
      type = lib.types.str;
      default = "/run/nura/var/lib/fingerprintd/sfs";
      description = "Original SFS tree. Only tzstorage/misc are exposed, read-only; original templates are not copied.";
    };
    persistMountPoint = lib.mkOption {
      type = lib.types.str;
      default = "/run/fp6-fingerprint-persist";
      description = "Existing persist mount protected read-only within this daemon; this module never mounts it.";
    };
    startAtBoot = lib.mkEnableOption "the read-only daemon at boot after separate hardware validation";
    allowExplicitRemoval = lib.mkEnableOption "explicit user-requested deletion APIs in enrollment mode (default disabled)";
    standbyUnlock = lib.mkEnableOption "direct logind standby unlock after verified enrollment and matching";
    enrollmentAndMatchVerified = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Records that actual physical enrollment and matching were verified; required for standbyUnlock. This module performs neither.";
    };
    enrollmentWindowSeconds = lib.mkOption {
      type = lib.types.ints.between 30 600;
      default = 300;
      description = "Bounded manual enrollment service lifetime; starting the service does not start enrollment.";
    };
  };
  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = !config.services.fprintd.enable
          && !(config.services.fp6Fingerprint.enable or false)
          && !(config.services.fp6FingerprintReadOnly.enable or false);
        message = "Preserved FP6 fingerprint integration requires all other fingerprint daemon modules disabled.";
      }
      {
        assertion = !enrollment || (!cfg.startAtBoot && !cfg.standbyUnlock);
        message = "The writable enrollment mode is manual, bounded and cannot unlock standby.";
      }
      {
        assertion = !cfg.allowExplicitRemoval || enrollment;
        message = "Explicit removal requires the separately selected enrollment mode.";
      }
      {
        assertion = !cfg.standbyUnlock || cfg.enrollmentAndMatchVerified;
        message = "Standby unlock requires actual physical enrollment and match evidence.";
      }
      {
        assertion = lib.all (p: lib.hasPrefix "/" p && !(lib.hasPrefix state p))
          [ cfg.trustlet cfg.legacySfsRoot cfg.persistMountPoint ];
        message = "Original fingerprint runtime paths must be absolute and outside the private NixOS state tree.";
      }
    ];
    environment.systemPackages = [ cfg.package ];
    services.dbus.packages = [ cfg.package ];
    system.build.fp6-fingerprint-preserved = cfg.package;
    systemd.tmpfiles.rules = map
      (suffix: "d ${state}${suffix} 0700 root root -")
      [ "" "/sfs" "/sfs/tzstorage" "/sfs/misc" "/sfs/persist-data" "/sfs/root3" ];
    systemd.services.fp6-fingerprint-preserved = {
      description = "FP6 fingerprint daemon with preserved original storage (${cfg.mode})";
      wantedBy = lib.optional cfg.startAtBoot "multi-user.target";
      requires = [ "dbus.service" ];
      after = [ "dbus.service" "systemd-modules-load.service" ];
      unitConfig = {
        ConditionPathExists = [ cfg.trustlet "/dev/tee0" "/dev/bsg/0:0:0:49476" ];
        ConditionPathIsMountPoint = cfg.persistMountPoint;
        ConditionPathIsDirectory = [ "${cfg.legacySfsRoot}/tzstorage" "${cfg.legacySfsRoot}/misc" ];
      };
      serviceConfig = {
        Type = "simple";
        ExecStartPre = prerequisites;
        ExecStart = lib.escapeShellArgs args;
        StateDirectory = "fp6-fingerprint-preserved";
        StateDirectoryMode = "0700";
        UMask = "0077";
        ProtectSystem = "strict";
        PrivateTmp = true;
        NoNewPrivileges = true;
        ReadWritePaths = [ state ];
        ReadOnlyPaths = [ cfg.persistMountPoint (builtins.dirOf cfg.legacySfsRoot) ]
          ++ lib.optional (!enrollment) "${state}/sfs";
        BindReadOnlyPaths = [
          "${cfg.legacySfsRoot}/tzstorage:${state}/sfs/tzstorage"
          "${cfg.legacySfsRoot}/misc:${state}/sfs/misc"
        ] ++ lib.optional (!enrollment) "${state}/sfs/persist-data:${state}/sfs/root3";
        # Root2 and root3 are aliases of one new private template directory.
        # No writable alias points at Android persist or the original SFS.
        BindPaths = lib.optional enrollment "${state}/sfs/persist-data:${state}/sfs/root3";
        Restart = "no";
        RuntimeMaxSec = if enrollment then cfg.enrollmentWindowSeconds else "infinity";
        KillMode = "mixed";
        TimeoutStopSec = 15;
      };
    };
    # Deliberately no PAM, D-Bus activation, learning, gestures or agent autostart.
    # Upstream authorizes caller UID/root; its installed polkit XML is not a
    # reauthentication implementation. Keep writable mode manually bounded.
  };
}
