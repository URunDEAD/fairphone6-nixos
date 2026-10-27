let
  pkgs = import <nixpkgs> {};
  system = import (pkgs.path + "/nixos") {
    configuration = {
      imports = [ ../module.nix ];
      hardware.fairphone6.quickShare = {
        enable = true;
        wifiInterfaces = [ "wlan0" "wlp2s0" ];
        # Avoid a package build; this test checks module configuration only.
        package = pkgs.hello;
      };
      system.stateVersion = "26.05";
    };
  };
  c = system.config;
in assert c.networking.firewall.allowedTCPPorts == [];
assert c.networking.firewall.allowedUDPPorts == [];
assert c.networking.firewall.interfaces.wlan0.allowedTCPPorts == [ 53317 ];
assert c.networking.firewall.interfaces.wlp2s0.allowedUDPPorts == [ 5353 ];
assert !(c.networking.firewall.interfaces ? eth0);
assert c.services.avahi.openFirewall == false;
assert c.services.avahi.settings.server.allow-interfaces == [ "wlan0" "wlp2s0" ];
assert c.systemd.user.services.fp6-quickshare.serviceConfig.UMask == "0077";
assert c.systemd.user.services.fp6-quickshare.serviceConfig.Type == "dbus";
assert c.systemd.user.services.fp6-quickshare.serviceConfig.BusName == "com.fairphone6.QuickShare1";
"Wi-Fi-only firewall, mDNS and user service assertions passed"
