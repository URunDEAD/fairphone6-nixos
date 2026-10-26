# Evaluation-only check of sensors-lomiri.nix on a minimal Lomiri system.
#   nix-instantiate --eval --strict --json nixos-fp6/feature-push/sensors/eval-check.nix
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  config = (import (nixpkgs + "/nixos") {
    system = "aarch64-linux";
    configuration = {
      imports = [ ./sensors-lomiri.nix ];
      system.stateVersion = "26.11";
      fileSystems."/" = { device = "none"; fsType = "tmpfs"; };
      boot.loader.grub.enable = false;
      boot.loader.generic-extlinux-compatible.enable = true;
      users.users.user = { uid = 10000; isNormalUser = true; };
      services.desktopManager.lomiri.enable = true;
      services.fp6LomiriSensors.enable = true;
    };
  }).config;
  svc = config.systemd.services.fp6-lomiri-powerd;
in {
  assertions = builtins.all (a: a.assertion) config.assertions;
  iioSensors = config.hardware.fp6IioSensors.enable;
  execStart = svc.serviceConfig.ExecStart;
  readWrite = svc.serviceConfig.ReadWritePaths;
  hfdWantedBy = config.systemd.services.hfd-service.wantedBy;
  accelRule = builtins.any (p: (p.name or "") == "81-fp6-accel-poll.rules") config.services.udev.packages;
  sensorsConf = config.environment.etc."xdg/QtProject/Sensors.conf".source;
  qtmirHasOrientationPatch = builtins.any (p: builtins.match ".*qtmir-orientation-owner-thread.patch" (toString p) != null)
    (import nixpkgs { system = "aarch64-linux"; overlays = config.nixpkgs.overlays; }).lomiri.qtmir.patches;
}
