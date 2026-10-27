{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.bugreport;
in {
  options.hardware.fairphone6.bugreport = {
    enable = lib.mkEnableOption "private Fairphone 6 diagnostics and Lomiri launcher";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix {};
      description = "fp6-bugreport package with the GUI enabled";
    };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    security.polkit.enable = true;
    # polkit discovers package policies through the system profile.
    environment.pathsToLink = [ "/share/polkit-1" ];
    systemd.user.services.fp6-bugreport = {
      description = "Create a private FP6 diagnostics report";
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      # Explicitly launched by the desktop entry; no collection or UI at login.
      serviceConfig = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/fp6-bugreport-gui";
        UMask = "0077";
        Restart = "no";
        TimeoutStopSec = 5;
        # pkexec needs to cross privilege boundaries after interactive authorization;
        # NoNewPrivileges/RestrictSUIDSGID would prevent that deliberate opt-in.
      };
    };
  };
}
