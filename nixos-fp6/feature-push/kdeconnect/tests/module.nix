{ pkgs ? import <nixpkgs> {} }:
let
  evaluated = import (pkgs.path + "/nixos/lib/eval-config.nix") {
    system = pkgs.stdenv.hostPlatform.system;
    modules = [
      ../module.nix
      ({ ... }: {
        system.stateVersion = "26.05";
        users.users.synthetic = { isNormalUser = true; };
        hardware.fairphone6.kdeconnect = {
          enable = true;
          user = "synthetic";
          wifiInterfaces = [ "wlan0" "wlp1s0" ];
          allowUinput = true;
          package = pkgs.emptyDirectory;
        };
      })
    ];
  };
  c = evaluated.config;
  service = c.systemd.user.services.lomiri-kdeconnect;
  clipboard = c.systemd.user.services.lomiri-kdeconnect-clipboard;
  bounded = unit: unit.serviceConfig.Restart == "on-failure"
    && unit.serviceConfig.RestartSec >= 5
    && unit.serviceConfig.RestartMaxDelaySec >= 60
    && unit.unitConfig.StartLimitBurst <= 10
    && unit.unitConfig.StartLimitIntervalSec >= 600;
in
assert builtins.elem "/share/lomiri-content-hub/peers" c.environment.pathsToLink;
assert c.networking.firewall.allowedTCPPorts == [];
assert c.networking.firewall.allowedUDPPorts == [];
assert c.networking.firewall.interfaces.wlan0.allowedUDPPorts == [ 1716 ];
assert c.networking.firewall.interfaces.wlp1s0.allowedTCPPortRanges == [ { from = 1716; to = 1764; } ];
assert service.unitConfig.ConditionUser == "synthetic";
# Regression (r4): getifaddrs() needs netlink; without it the daemon exited
# with EAFNOSUPPORT on every start and systemd restarted it every 5 s.
assert builtins.elem "AF_NETLINK" service.serviceConfig.RestrictAddressFamilies;
assert bounded service;
assert bounded clipboard;
assert builtins.elem "graphical-session.target" service.wantedBy;
assert builtins.elem "kdeconnect-input" c.users.users.synthetic.extraGroups;

{
  inherit (service) serviceConfig;
  firewall = c.networking.firewall.interfaces;
  uinput = c.services.udev.extraRules;
}
