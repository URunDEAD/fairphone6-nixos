let
  pkgs = import <nixpkgs> {};
  evaluate = enabled: import (pkgs.path + "/nixos/lib/eval-config.nix") {
    system = "x86_64-linux";
    modules = [ ../module.nix {
      hardware.fairphone6.captions.enable = enabled;
      hardware.fairphone6.captions.package = pkgs.hello;
      system.stateVersion = "26.05";
    } ];
  };
  on = (evaluate true).config;
  off = (evaluate false).config;
  service = on.systemd.user.services.fp6-captions;
in assert service.serviceConfig.Type == "dbus";
assert service.serviceConfig.BusName == "com.fairphone6.Captions1";
assert service.serviceConfig.RestrictAddressFamilies == [ "AF_UNIX" ];
assert service.serviceConfig.LimitCORE == 0;
assert service.serviceConfig.CPUQuota == "200%";
assert on.services.pipewire.enable;
assert !(off.systemd.user.services ? fp6-captions);
"module enable/disable and privacy/resource bounds passed"
