{ pkgs ? import <nixpkgs> {}, system ? builtins.currentSystem }:
let
  evaluated = import (pkgs.path + "/nixos/lib/eval-config.nix") {
    inherit system;
    modules = [ ../module.nix {
      system.stateVersion = "26.05";
      users.users.phablet = { isNormalUser = true; uid = 1000; };
      hardware.fairphone6.netpolicy = {
        enable = true;
        apps = [{ id = "org.example.Chat"; unit_pattern = "app-org.example.Chat-*.scope"; }];
      };
    } ];
  };
  cfg = evaluated.config;
  user = cfg.systemd.user.services.fp6-netpolicy;
  helper = cfg.systemd.services.fp6-netpolicy-helper;
in
assert cfg.hardware.fairphone6.netpolicy.enable;
assert user.unitConfig.ConditionUser == "phablet";
assert helper.serviceConfig.ProtectControlGroups;
assert ! (helper.serviceConfig.PrivateNetwork or false);
{
  inherit system;
  userExec = user.serviceConfig.ExecStart;
  helperExec = helper.serviceConfig.ExecStart;
  helperCapabilities = helper.serviceConfig.CapabilityBoundingSet;
  dbusPolicy = map toString cfg.services.dbus.packages;
}
