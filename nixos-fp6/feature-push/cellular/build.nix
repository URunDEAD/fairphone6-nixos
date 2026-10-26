# Build entry for the cellular feature push userspace (ARM builder):
#   .macmini-builder/nix-remote build --eval-store daemon --file nixos-fp6/feature-push/cellular/build.nix --no-link --print-out-paths
let
  pin = /nix/store/82jdygd6crh2575x13mdqjmqds5ffwfs-source;
  pkgs = import pin { system = "aarch64-linux"; config.allowUnfree = true; };
  fp6 = import ./packages.nix { inherit pkgs; };
in {
  inherit (fp6) tqftpserv fp6-modem-mcfg-activate fp6-modem-data-port ofono fp6-loc-nmea fp6-location-indicator msm-modem-uim-selection fp6-ofono-account-wrapped lomiri-system-settings-cellular;
}
