let
  pkgs = import <nixpkgs> {};
  evaluate = enabled: import (pkgs.path + "/nixos/lib/eval-config.nix") {
    system = "x86_64-linux";
    modules = [ ../module.nix {
      hardware.fairphone6.dictation.enable = enabled;
      # Evaluate module behavior without building the package.
      hardware.fairphone6.dictation.package = pkgs.hello;
      system.stateVersion = "26.05";
    } ];
  };
  on = (evaluate true).config;
  off = (evaluate false).config;
  service = on.systemd.user.services.fp6-dictation;
in assert service.serviceConfig.Type == "dbus";
assert service.serviceConfig.BusName == "com.fairphone6.Dictation1";
assert service.serviceConfig.RestrictAddressFamilies == [ "AF_UNIX" ];
assert service.serviceConfig.MemoryMax == "500M";
assert on.services.pipewire.enable;
assert !(off.systemd.user.services ? fp6-dictation);
"module enable/disable and service assertions passed"
