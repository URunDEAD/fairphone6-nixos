{ pkgs ? import <nixpkgs> {} }:
let
  eval = import (pkgs.path + "/nixos/lib/eval-config.nix") {
    inherit (pkgs.stdenv.hostPlatform) system;
    modules = [ ../module.nix ({ ... }: {
      hardware.fairphone6.bugreport.enable = true;
      system.stateVersion = "26.05";
      fileSystems."/" = { device = "/dev/synthetic"; fsType = "ext4"; };
      boot.loader.grub.enable = false;
    }) ];
  };
  cfg = eval.config;
in assert cfg.security.polkit.enable;
assert cfg.systemd.user.services.fp6-bugreport.serviceConfig.UMask == "0077";
assert cfg.systemd.user.services.fp6-bugreport.wantedBy == [];
assert builtins.elem "/share/polkit-1" cfg.environment.pathsToLink;
{ exec = cfg.systemd.user.services.fp6-bugreport.serviceConfig.ExecStart;
  enabled = cfg.hardware.fairphone6.bugreport.enable;
  failedAssertions = map (a: a.message) (builtins.filter (a: !a.assertion) cfg.assertions);
}
