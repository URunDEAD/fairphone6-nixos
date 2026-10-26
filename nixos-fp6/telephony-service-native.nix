{ lib, pkgs, ... }:
{
  nixpkgs.overlays = lib.mkAfter [ (import ./telephony-service-native-overlay.nix) ];
  # Make D-Bus SystemdService lookups independent of profile XDG discovery.
  # The handler is bus-activated and has no Install/autostart target.
  systemd.packages = [ pkgs.lomiri.lomiri-telephony-service ];
}
