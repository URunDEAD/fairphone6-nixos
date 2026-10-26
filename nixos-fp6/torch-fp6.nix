{ config, lib, pkgs, ... }:
let cfg = config.hardware.fp6Torch;
in {
  options.hardware.fp6Torch.enable = lib.mkEnableOption "FP6 PM7550 torch access for the video group";
  config = lib.mkIf cfg.enable {
    # Ayatana's existing simple torch backend already recognizes white:flash.
    # Grant access only to its continuous-brightness attribute. Flash current,
    # strobe, timeout and all other LED devices retain their default permissions.
    services.udev.extraRules = ''
      ACTION=="add|change", SUBSYSTEM=="leds", KERNEL=="white:flash", KERNELS=="c400000.spmi:pmic@1:led-controller@ee00", DRIVERS=="leds-qcom-flash", RUN+="${pkgs.coreutils}/bin/chgrp video /sys%p/brightness", RUN+="${pkgs.coreutils}/bin/chmod 0664 /sys%p/brightness"
    '';
  };
}
