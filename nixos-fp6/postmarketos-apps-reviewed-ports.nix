# Optional add-on to the package-only native application migration.
# Installing LPA/VVM frontends performs no SIM/voicemail/account migration.
{ config, lib, pkgs, ... }:
let ports = import ./postmarketos-apps-ports.nix { inherit pkgs; };
in {
  imports = [ ./postmarketos-apps.nix ];
  hardware.fairphone6.postmarketosApps.extraPackages =
    lib.mkIf config.hardware.fairphone6.postmarketosApps.enable ports.packages;
}
