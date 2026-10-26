{ config, lib, pkgs, ... }:
# FP6 offline charging (charger-triggered boot) for a PERSISTED NixOS boot image.
#
# Bootloader behaviour (confirmed on-device 2026-08-24 by the community
# charging-mode work, preserved in the original pmOS root
# /run/nura/usr/lib/systemd/system-generators/charging-mode-generator):
# when the phone is OFF and a charger is inserted, ABL PONs with reason
# USB_CHARGER and boots the ACTIVE boot partition (boot_b) with
# "androidboot.mode=charger" appended to the cmdline. A warm reboot PONs as
# HARD_RESET, so ABL does NOT add mode=charger and the next boot is normal.
#
# Today boot_b is the ORIGINAL postmarketOS image, whose root already has a
# working charging.target (charging-monitor/-keys/-usbnet). Nothing changes
# until a NixOS image is persisted to boot_b (not authorised). This module
# is the NixOS equivalent for that day. It is inert on normal boots: the
# generator only acts when androidboot.mode=charger is on /proc/cmdline.
#
# Deliberate differences from the pmOS r5 prototype:
#  * NO suspend cycling (FP6 UFS hibern8-exit fails on system resume on the
#    current kernel; see power-suspend-policy-fp6.nix);
#  * NO UFS auto_hibern8 writes (prime suspect of the 2026-08-24 crash);
#  * NO rfkill (systemd-rfkill would persist a soft-block into normal boots);
#  * escape hatch file /var/lib/fp6-charging-mode/disable (state dir, since
#    /etc is generated on NixOS).
let
  cfg = config.services.fp6ChargerMode;
  monitor = pkgs.writeShellApplication {
    name = "fp6-charging-monitor";
    runtimeInputs = [ pkgs.coreutils pkgs.systemd ];
    text = ''
      BAT=/sys/class/power_supply/qcom-battmgr-bat
      USB=/sys/class/power_supply/qcom-battmgr-usb
      log() { echo "fp6-charging-monitor: $*"; }
      log "offline charging mode active"
      i=0
      while [[ ! -r "$USB/online" ]]; do
        i=$((i + 1))
        if (( i > 180 )); then log "battmgr absent after ''${i}s: failsafe reboot to a normal boot"; exec systemctl reboot; fi
        sleep 1
      done
      log "battmgr up after ''${i}s"
      for b in /sys/class/backlight/*/brightness; do [[ -w "$b" ]] && echo 0 > "$b" 2>/dev/null || true; done
      for f in /sys/class/graphics/fb*/blank; do [[ -w "$f" ]] && { echo 4 > "$f" || echo 1 > "$f"; } 2>/dev/null || true; done
      for r in /sys/class/remoteproc/remoteproc*; do
        name=$(cat "$r/name" 2>/dev/null || true)
        case "$name" in
          *mpss*|*modem*|*cdsp*) timeout 25 sh -c "echo stop > '$r/state'" 2>/dev/null || true
                                 log "remoteproc $name -> $(cat "$r/state" 2>/dev/null || echo ?)" ;;
        esac
      done
      ${lib.optionalString cfg.capCpu ''
        for p in /sys/devices/system/cpu/cpufreq/policy*; do cat "$p/cpuinfo_min_freq" > "$p/scaling_max_freq" 2>/dev/null || true; done
      ''}
      tick=0
      while :; do
        n=0
        while [[ "$(cat "$USB/online" 2>/dev/null || echo 1)" == 0 ]]; do
          n=$((n + 1))
          if (( n >= 3 )); then log "charger removed (capacity $(cat "$BAT/capacity" 2>/dev/null)%): power off"; exec systemctl poweroff; fi
          sleep 3
        done
        if (( tick % 12 == 0 )); then
          log "sample cap=$(cat "$BAT/capacity" 2>/dev/null)% status=$(cat "$BAT/status" 2>/dev/null) ibat=$(cat "$BAT/current_now" 2>/dev/null)uA vbus=$(cat "$USB/voltage_now" 2>/dev/null)uV"
        fi
        tick=$((tick + 1))
        sleep 5
      done
    '';
  };
  generator = pkgs.writeShellScript "fp6-charging-mode-generator" ''
    # Must never break a normal boot: every failure path exits 0.
    [ -e /var/lib/fp6-charging-mode/disable ] && exit 0
    ${pkgs.gnugrep}/bin/grep -Eq '(^| )androidboot\.mode=charger( |$)' /proc/cmdline || exit 0
    early="''${2:-$1}"
    [ -n "$early" ] && [ -d "$early" ] || exit 0
    ln -sf ${placeholderTarget} "$early/default.target" || exit 0
    : > /run/fp6-charging-mode 2>/dev/null
    mkdir -p /run/systemd/logind.conf.d 2>/dev/null && \
      printf '[Login]\nHandlePowerKey=reboot\nHandlePowerKeyLongPress=reboot\n' > /run/systemd/logind.conf.d/10-fp6-charging-mode.conf
    exit 0
  '';
  placeholderTarget = "/etc/systemd/system/fp6-charging.target";
  skipInCharging = [ "bluetooth" "iio-sensor-proxy" "NetworkManager" "ModemManager" "waydroid-container" "display-manager" "rmtfs" ];
in {
  options.services.fp6ChargerMode = {
    enable = lib.mkEnableOption "FP6 offline-charging target for charger-triggered (androidboot.mode=charger) boots";
    capCpu = lib.mkOption { type = lib.types.bool; default = true; };
  };
  config = lib.mkIf cfg.enable {
    systemd.generators.fp6-charging-mode = generator;
    systemd.targets.fp6-charging = {
      description = "FP6 offline charging mode";
      requires = [ "basic.target" ];
      after = [ "basic.target" "rescue.service" "rescue.target" ];
      conflicts = [ "rescue.service" "rescue.target" ];
      wants = [ "fp6-charging-monitor.service" "systemd-logind.service" ];
      unitConfig.AllowIsolate = true;
    };
    # Keep radios/UI out of charger boots; inert in normal boots (no marker).
    systemd.services = lib.mkMerge ([{
      fp6-charging-monitor = {
        description = "FP6 offline charging monitor";
        after = [ "basic.target" ];
        unitConfig.ConditionPathExists = "/run/fp6-charging-mode";
        serviceConfig = { Type = "exec"; ExecStart = lib.getExe monitor; Restart = "on-failure"; RestartSec = 5; };
      };
    }]);
    # Drop-ins (harmless for units that do not exist on a given generation).
    systemd.packages = [ (pkgs.runCommand "fp6-charging-mode-dropins" { } (lib.concatMapStrings (u: ''
      mkdir -p $out/lib/systemd/system/${u}.service.d
      printf '[Unit]\nConditionPathExists=!/run/fp6-charging-mode\n' > $out/lib/systemd/system/${u}.service.d/zz-fp6-charging-mode.conf
    '') skipInCharging)) ];
    systemd.tmpfiles.rules = [ "d /var/lib/fp6-charging-mode 0755 root root -" ];
    system.build.fp6-charging-monitor = monitor;
  };
}
