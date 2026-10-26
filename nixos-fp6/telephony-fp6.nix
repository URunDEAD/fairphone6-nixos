{ config, lib, pkgs, ... }:
let
  cfg = config.services.fp6TelephonyMiddleware;
  ofono-qt = pkgs.libsForQt5.callPackage ./ofono-qt-package.nix { };
in {
  options.services.fp6TelephonyMiddleware = {
    enable = lib.mkEnableOption "native UBports Telepathy oFono middleware (no modem or account activation)";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.libsForQt5.callPackage ./telepathy-ofono-package.nix { inherit ofono-qt; };
      description = "Telepathy connection manager only; requires a separately validated oFono-compatible backend and explicit account creation.";
    };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    services.dbus.packages = [ cfg.package ];
    system.build.fp6-telepathy-ofono = cfg.package;
    # No modem service, Android account plugin, fake SIM/registration
    # properties, automatic account creation, IMS registration or PIN action.
  };
}
