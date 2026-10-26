# Optional native apps only. This module does not supply a working modem,
# create an account, enable telephony middleware or activate online accounts.
{ config, lib, pkgs, ... }:
let
  cfg = config.programs.fp6TelephonyFrontends;
  packages = import ./telephony-frontends-packages.nix { inherit pkgs; };
in {
  options.programs.fp6TelephonyFrontends = {
    enable = lib.mkEnableOption "native Lomiri Contacts, dialer and messaging applications";
    packages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [ packages.addressbook packages.dialer packages.messaging ];
      description = "Applications with real QtContacts SQLite and Lomiri QML bindings; account/backend setup remains separate.";
    };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = cfg.packages;
  };
}
