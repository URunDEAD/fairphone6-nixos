# Persistent read-only EFS shadow for the FP6 modem (rmtfs -r -D).
#
# Problem: rmtfs runs read-only (-r): every modem EFS write lives in a RAM
# shadow and is gone at power-off, so on every boot the modem re-selects its
# platform MCFG, leaves it Pending, and fp6-modem-mcfg-activate has to
# restart it (~70 s into boot; oFono, and with it the phone's whole mobile
# network, waits for that).
#
# Fix (coordinator decision 2026-10-27, user delegated the choice): rmtfs
# keeps the partitions strictly READ-ONLY and saves its RAM shadow as files
# in /var/lib/rmtfs-shadow (0700), loading them at the next start when they
# are valid for the same source partition. First-capture copies of the
# partitions are kept in /var/lib/rmtfs-shadow/original/ (0400).
#   * rmtfs patch: ./0001-rmtfs-persist-read-only-shadow.patch (-D <dir>,
#     refused without -r; tests: ./test_shadow.c, run in checkPhase).
#   * /run/tqftpserv (the modem's TFTP /readwrite/ scratch, incl. mcfg.tmp)
#     points into the same directory and lives exactly as long as the
#     images (a stale mcfg.tmp next to a fresh EFS makes the modem skip
#     MCFG selection; a fresh scratch next to a saved EFS is the case this
#     keeps from happening).
#   * Fallback: rmtfs ExecStartPre counts starts with saved images;
#     fp6-rmtfs-shadow-confirm clears the count once the platform MCFG is
#     Active. After 2 unconfirmed starts the images are moved to
#     quarantine/ and the modem starts from the partitions (today's
#     behaviour, one activation restart).
#   * Reset: `fp6-rmtfs-shadow reset` (root), then reboot.
#   * fp6-modem-mcfg-activate is unchanged: it only restarts the modem when
#     a platform config is Pending and none is Active.
# Default OFF until the supervised phone test passed.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.modem;
  dir = "/var/lib/rmtfs-shadow";

  rmtfs-shadow = pkgs.rmtfs.overrideAttrs (old: {
    pname = "rmtfs-fp6-shadow";
    __intentionallyOverridingVersion = true;
    patches = (old.patches or [ ]) ++ [ ./0001-rmtfs-persist-read-only-shadow.patch ];
    doCheck = true;
    checkPhase = ''
      runHook preCheck
      cp ${./test_shadow.c} test_shadow.c
      $CC -Wall -Werror -O2 -I. -o test_shadow test_shadow.c storage.c shadow.c
      TMPDIR=$(mktemp -d) ./test_shadow
      runHook postCheck
    '';
  });

  tool = pkgs.writeShellApplication {
    name = "fp6-rmtfs-shadow";
    runtimeInputs = [ pkgs.coreutils pkgs.findutils pkgs.gnugrep pkgs.gawk pkgs.libqmi ];
    text = builtins.readFile ./fp6-rmtfs-shadow.sh;
  };

  toolTest = pkgs.runCommand "fp6-rmtfs-shadow-test" {
    nativeBuildInputs = [ pkgs.bash pkgs.coreutils pkgs.findutils pkgs.gnugrep ];
  } ''
    bash ${./test-fp6-rmtfs-shadow.sh} ${lib.getExe tool} | tee $out
  '';

  rmtfsExec = "${rmtfs-shadow}/bin/rmtfs -r -P -s -D ${dir}";
in {
  options.hardware.fairphone6.modem.persistShadow = lib.mkOption {
    type = lib.types.bool;
    default = false;
    description = ''
      Keep the modem EFS partitions read-only, but persist rmtfs's RAM
      shadow in ${dir} so the modem's MCFG selection survives reboots and
      the per-boot activation restart disappears. Reset with
      `fp6-rmtfs-shadow reset`; falls back automatically after 2 starts
      that never confirmed.
    '';
  };

  config = lib.mkMerge [
    {
      # Built (and tested) even while the option is off.
      system.build.fp6-rmtfs-shadow = { rmtfs = rmtfs-shadow; inherit tool toolTest; };
    }
    (lib.mkIf cfg.persistShadow {
      assertions = [
        { assertion = config.services.fp6Cellular.enable or false;
          message = "hardware.fairphone6.modem.persistShadow belongs to the fp6Cellular stack."; }
        { assertion = lib.hasInfix " -r " config.systemd.services.rmtfs.serviceConfig.ExecStart;
          message = "persistShadow: rmtfs must stay read-only (-r)."; }
      ];

      systemd.services.rmtfs = {
        serviceConfig = {
          ExecStart = lib.mkForce rmtfsExec;
          # Replaces cellular-fp6.nix's "rm -rf /run/tqftpserv": the scratch
          # now follows the saved images (see fp6-rmtfs-shadow prestart).
          ExecStartPre = lib.mkForce [ "-${lib.getExe tool} prestart" ];
          StateDirectory = "rmtfs-shadow";
          StateDirectoryMode = "0700";
        };
      };

      systemd.services.fp6-rmtfs-shadow-confirm = {
        description = "FP6 modem: confirm the saved EFS shadow (platform MCFG Active)";
        wantedBy = [ "multi-user.target" "rmtfs.service" ];
        partOf = [ "rmtfs.service" ];
        after = [ "rmtfs.service" "fp6-modem-mcfg-activate.service" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "${lib.getExe tool} confirm";
          TimeoutStartSec = 300;
        };
      };

      environment.systemPackages = [ tool ];
    })
  ];
}
