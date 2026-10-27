{ pkgs ? import <nixpkgs> {}, system ? builtins.currentSystem }:
let
  cfg = (import (pkgs.path + "/nixos/lib/eval-config.nix") {
    inherit system;
    modules = [ ../module.nix {
      system.stateVersion = "26.05";
      users.users.phablet = { isNormalUser = true; uid = 1000; };
      hardware.fairphone6.selftest.enable = true;
    } ];
  }).config;
  user = cfg.systemd.user.services.fp6-selftest;
  systemUnit = cfg.systemd.services.fp6-selftest-system;
in
assert user.unitConfig.ConditionUser == "phablet";
assert systemUnit.serviceConfig.User == "phablet";
assert systemUnit.serviceConfig.RestrictAddressFamilies == [ "AF_UNIX" ];
assert user.serviceConfig.UMask == "0077";
assert cfg.systemd.user.timers.fp6-selftest.timerConfig.OnActiveSec == "90s";
assert cfg.xdg.mime.defaultApplications."x-scheme-handler/fp6-selftest" == "com.fairphone6.selftest.desktop";
{ inherit system; userExec = user.serviceConfig.ExecStart; systemExec = systemUnit.serviceConfig.ExecStart; package = cfg.hardware.fairphone6.selftest.package.drvPath; }
