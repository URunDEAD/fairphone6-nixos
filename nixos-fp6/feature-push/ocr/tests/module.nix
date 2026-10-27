{ system ? builtins.currentSystem }:
let
  evaluated = import (<nixpkgs> + "/nixos/lib/eval-config.nix") {
    inherit system;
    modules = [ ../module.nix {
      system.stateVersion = "26.05";
      hardware.fairphone6.ocr.enable = true;
    } ];
  };
  config = evaluated.config;
  service = config.systemd.user.services.fairphone-ocr;
in assert config.hardware.fairphone6.ocr.enable;
assert service.serviceConfig.BusName == "com.fairphone6.Ocr1";
assert service.serviceConfig.Type == "dbus";
assert service.serviceConfig.MemoryMax == "384M";
assert builtins.elem config.hardware.fairphone6.ocr.package config.services.dbus.packages;
{
  platform = config.hardware.fairphone6.ocr.package.stdenv.hostPlatform.system;
  command = service.serviceConfig.ExecStart;
  wantedBy = service.wantedBy;
}
