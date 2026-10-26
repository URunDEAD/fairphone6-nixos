{ config, lib, pkgs, ... }:
# Follow-ups from the 2026-10-26 live quick checks on the Lomiri/kernel5 system
# (results/feature-push-20261024/quick-checks/README.md):
#
#  * Offline charging: boot_b now holds a NixOS Lomiri image, not the pmOS
#    one. An ABL charger PON (androidboot.mode=charger) would therefore boot
#    the full Lomiri session. charger-mode-fp6.nix was written for this case
#    (inert on normal boots) but the full target never imported it.
#  * MTP: mtp-fp6.nix (umtprd on FunctionFS, added next to the stage-1
#    NCM+ADB gadget) passed on the phone under Phosh, but the Lomiri target
#    does not import it. It is start-on-demand (systemctl start fp6-mtp). It
#    briefly unbinds the UDC, so USB ssh drops for about 1 s and comes back.
#    It is never started at boot.
#  * NFC: the Samsung S3NRN4V (s3fwrn5 driver, nfc0) is bound but there is no
#    userspace. Adds neard (D-Bus activatable daemon, not started at boot) and
#    its tools. Lomiri has no NFC UI, so this is plumbing only.
{
  imports = [ ./charger-mode-fp6.nix ../../mtp-fp6.nix ];

  options.hardware.fairphone6.quickChecks.enable =
    lib.mkEnableOption "FP6 quick-check follow-ups (offline charging target, on-demand MTP, neard)";

  config = lib.mkIf config.hardware.fairphone6.quickChecks.enable {
    services.fp6ChargerMode.enable = true;
    services.fp6Mtp.enable = true;

    environment.systemPackages = [ pkgs.neard ];
    systemd.packages = [ pkgs.neard ];
    services.dbus.packages = [ pkgs.neard ];
    # neard's upstream unit has no [Install] wants; it only starts when
    # something asks for org.neard on the system bus (or manually).
  };
}
