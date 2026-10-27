{ pkgs ? import <nixpkgs> {} }:
let
  eval = system: enabled: (import (pkgs.path + "/nixos/lib/eval-config.nix") {
    inherit system;
    modules = [ ../module.nix ({ ... }: {
      system.stateVersion = "26.05";
      hardware.fairphone6.share.enable = enabled;
      boot.isContainer = true;
      fileSystems."/" = { device = "none"; fsType = "tmpfs"; };
    }) ];
  }).config;
  on = eval "x86_64-linux" true;
  off = eval "x86_64-linux" false;
  arm = eval "aarch64-linux" true;
in
assert on.systemd.user.services.fp6-share.serviceConfig.BusName == "com.fairphone6.Share1";
assert on.systemd.user.services.fp6-share.serviceConfig.UMask == "0077";
assert on.systemd.user.services.fp6-share.partOf == [ "graphical-session.target" ];
assert on.xdg.mime.defaultApplications."x-scheme-handler/share" == "com.fairphone6.Share.desktop";
assert !(off.systemd.user.services ? fp6-share);
assert builtins.elem "/share/lomiri-content-hub/peers" on.environment.pathsToLink;
assert builtins.elem "aarch64-linux" arm.hardware.fairphone6.share.package.meta.platforms;
assert arm.hardware.fairphone6.share.package.stdenv.hostPlatform.system == "aarch64-linux";
{ enabled = true; disabled = true; aarch64Evaluation = true; }
