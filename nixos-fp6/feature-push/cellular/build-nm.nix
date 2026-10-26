# NetworkManager with -Dofono=true (separate: larger build).
let
  pin = /nix/store/82jdygd6crh2575x13mdqjmqds5ffwfs-source;
  pkgs = import pin { system = "aarch64-linux"; config.allowUnfree = true; };
in (import ./packages.nix { inherit pkgs; }).networkmanager-ofono
