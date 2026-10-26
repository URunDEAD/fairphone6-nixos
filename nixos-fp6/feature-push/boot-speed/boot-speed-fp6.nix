# FP6 boot speed (2026-10-26). No-op unless `enable` is set.
# Notes and the measured boot profile: results/feature-push-20261024/boot-speed/README.md
#
# Stage-1 (Mobile NixOS init): Tasks::Modules ran `modprobe spi_geni_qcom` in
# its blocking loop. That load binds the built-in ESWIN touchscreen, whose
# probe sleeps 5 s (msleep(5000), "wait ic boot done"), so every later stage-1
# task (DRM/fbdev, gadget, udev, root image, splash, switch_root) started
# 5.4 s late on kernel6 (dmesg: eph_probe 1.257 s -> 6.656 s).
# backgroundModules are still loaded by the stage-1 init, but through a
# spawned modprobe, so the sleep overlaps the rest of stage-1. The
# touchscreen and IMU (spi1.0) are still bound before stage-2 starts. Nothing in
# stage-2 changes: fp6-lomiri-powerd (lp_mode/double-tap ReadWritePaths) and
# Lomiri find the same devices at the same point in stage-2.
#
# The Mobile NixOS recovery menu is unaffected. It is opened by holding a
# volume key during stage-1 (gpio-keys and pmic pwrkey/resin are built in,
# and evdev is loaded in the foreground as before) and is driven with the volume/power keys. If the menu
# starts before the touchscreen has finished probing, only touch is missing.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.bootSpeed;
  stage1Modules = config.mobile.boot.stage-1.kernel.modules;
  background = lib.filter (m: lib.elem m stage1Modules) cfg.backgroundModules;
  rubyList = "[ " + lib.concatMapStringsSep ", " (m: ''"${m}"'') background + " ]";
  backgroundTask = pkgs.writeText "fp6-boot-speed-background-modules.rb"
    (builtins.replaceStrings [ "@backgroundModules@" ] [ rubyList ]
      (builtins.readFile ./background-modules.rb));
  # Stage-1 only: udev (80-drivers.rules -> kmod builtin) must not load these
  # by modalias while the background modprobe is in the kernel. Otherwise
  # `udevadm settle` waits for it. Blacklisting applies to alias loads only.
  stage1Modprobe = pkgs.writeText "fp6-boot-speed-modprobe.conf"
    (lib.concatMapStrings (m: "blacklist ${m}\n") background);
in
{
  options.hardware.fairphone6.bootSpeed = {
    enable = lib.mkEnableOption "FP6 boot-time optimizations (stage-1 background module loading)";
    backgroundModules = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "spi_geni_qcom" ];
      description = ''
        Stage-1 kernel modules (from mobile.boot.stage-1.kernel.modules) that
        are loaded by a background modprobe instead of the blocking stage-1
        Tasks::Modules loop. Use this for modules whose probe sleeps.
        Entries not in the stage-1 module list are ignored.
      '';
    };
  };

  config = lib.mkIf (cfg.enable && background != [ ]) {
    mobile.boot.stage-1.tasks = [ backgroundTask ];
    mobile.boot.stage-1.contents = [
      { object = stage1Modprobe; symlink = "/etc/modprobe.d/fp6-boot-speed.conf"; }
    ];
  };
}
