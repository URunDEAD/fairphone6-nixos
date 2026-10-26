# Evaluate the module against a minimal aarch64 NixOS and expose buildable
# pieces. Build on the ARM builder:
#   .macmini-builder/nix-remote build --eval-store daemon \
#     --file nixos-fp6/feature-push/network/build.nix pkg neardConf dbusConf testTools --no-link --print-out-paths
# Evaluation-only checks: nix-instantiate --eval --strict --json -A checks build.nix
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  eval = extra: import (nixpkgs + "/nixos") {
    system = "aarch64-linux";
    configuration = {
      imports = [ ./network-fp6.nix ];
      system.stateVersion = "26.11";
      boot.loader.grub.enable = false;
      fileSystems."/" = { device = "none"; fsType = "tmpfs"; };
      networking.nftables.enable = true;
      hardware.fairphone6.network = { enable = true; } // extra;
    };
  };
  default = eval { };
  full = eval { nfc.enable = true; wifiCountry = "RO"; flightMode.modemBackend = "modemmanager"; };
  c = default.config;
  f = full.config;
  units = cfg: cfg.systemd.services;
in
{
  checks = {
    btEnabled = c.hardware.bluetooth.enable;
    wireplumberBluez = c.services.pipewire.wireplumber.extraConfig."51-fp6-bluez";
    firewallWlan0 = c.networking.firewall.interfaces.wlan0;
    urfkillExec = c.systemd.services.fp6-urfkill.serviceConfig.ExecStart;
    urfkillInDbus = builtins.length c.services.dbus.packages;
    neardDefaultOff = !c.services.neard.enable;
    neardFull = f.environment.etc."neard/main.conf".text or "generated";
    neardSettings = f.services.neard.settings;
    countryUnit = (units f) ? fp6-wifi-country;
    countryAbsentByDefault = !((units c) ? fp6-wifi-country);
    fullModem = f.systemd.services.fp6-urfkill.serviceConfig.ExecStart;
  };
  # Buildable aarch64 outputs: the daemon package (runs its own D-Bus check
  # in installCheckPhase) and generated configs of the full evaluation.
  pkg = default.pkgs.callPackage ./fp6-urfkill-package.nix { };
  neardConf = f.environment.etc."neard/main.conf".source;
  dbusConf = f.environment.etc."dbus-1".source;
  # Tools the live test needs that the current phone system lacks: hostapd for
  # the NM-independent concurrent STA+AP probe on ap0, pactl for BT audio.
  testTools = default.pkgs.buildEnv {
    name = "fp6-network-test-tools";
    paths = with default.pkgs; [ hostapd pulseaudio iw ];
    pathsToLink = [ "/bin" ];
  };
}
