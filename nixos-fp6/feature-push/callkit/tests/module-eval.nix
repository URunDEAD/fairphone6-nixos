let
  pkgs = import <nixpkgs> {};
  evaluated = import (pkgs.path + "/nixos/lib/eval-config.nix") {
    system = pkgs.stdenv.hostPlatform.system;
    modules = [ ../module.nix ({ ... }: {
      hardware.fairphone6.calls.enable = true;
      system.stateVersion = "26.05";
      boot.loader.grub.enable = false;
      fileSystems."/".device = "none";
    }) ];
  };
  svc = evaluated.config.systemd.user.services.fp6-calls;
in assert svc.serviceConfig.BusName == "com.fairphone6.Calls1";
   assert svc.serviceConfig.Type == "dbus";
   assert svc.serviceConfig.RestrictAddressFamilies == [ "AF_UNIX" ];
   assert svc.wantedBy == [ "graphical-session.target" ];
   { exec = svc.serviceConfig.ExecStart; wantedBy = svc.wantedBy; }
