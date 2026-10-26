{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs, withMediaplayer ? true }:
# Evaluate all misc feature modules in a minimal aarch64 NixOS config and
# build their tools + generated unit files (no full system, no kernel).
#   .macmini-builder/nix-remote build --eval-store daemon \
#     --file nixos-fp6/feature-push/misc/misc-build.nix --no-link --print-out-paths
let
  eval = (import (nixpkgs + "/nixos") {
    system = "aarch64-linux";
    configuration = {
      imports = [
        ./rtc-offset-fp6.nix ./sd-automount-fp6.nix ./power-suspend-policy-fp6.nix
        ./factory-reset-fp6.nix ./charger-mode-fp6.nix ./mediaplayer-hwdecode-fp6.nix
      ];
      system.stateVersion = "26.11";
      fileSystems."/" = { device = "none"; fsType = "tmpfs"; };
      boot.loader.grub.enable = false;
      boot.loader.generic-extlinux-compatible.enable = true;
      networking.networkmanager.enable = true;
      users.users.user = { uid = 10000; isNormalUser = true; extraGroups = [ "video" ]; };
      services.fp6RtcOffset.enable = true;
      services.fp6SdAutomount.enable = true;
      services.fp6SuspendPolicy.enable = true;
      services.fp6FactoryReset.enable = true;
      services.fp6ChargerMode.enable = true;
      services.fp6MediaplayerHwDecode.enable = withMediaplayer;
    };
  });
  c = eval.config;
  pkgs = eval.pkgs;
  failed = builtins.filter (a: !a.assertion) c.assertions;
  unit = n: c.systemd.units.${n}.unit;
in
assert failed == [ ] || throw (builtins.concatStringsSep "\n" (map (a: a.message) failed));
pkgs.linkFarm "fp6-feature-push-misc" ([
  { name = "bin/fp6-rtc-offset"; path = c.system.build.fp6-rtc-offset; }
  { name = "bin/fp6-sd-mount"; path = c.system.build.fp6-sd-mount; }
  { name = "bin/fp6-factory-reset"; path = c.system.build.fp6-factory-reset; }
  { name = "bin/fp6-charging-monitor"; path = c.system.build.fp6-charging-monitor; }
  { name = "units/fp6-rtc-offset-restore.service"; path = unit "fp6-rtc-offset-restore.service"; }
  { name = "units/fp6-rtc-offset-save.timer"; path = unit "fp6-rtc-offset-save.timer"; }
  { name = "units/fp6-sd-mount@.service"; path = unit "fp6-sd-mount@.service"; }
  { name = "units/fp6-factory-reset.service"; path = unit "fp6-factory-reset.service"; }
  { name = "units/fp6-charging.target"; path = unit "fp6-charging.target"; }
  { name = "units/fp6-charging-monitor.service"; path = unit "fp6-charging-monitor.service"; }
  { name = "etc/sleep.conf"; path = c.environment.etc."systemd/sleep.conf".source; }
  { name = "etc/UPower.conf"; path = c.environment.etc."UPower/UPower.conf".source; }
  { name = "udev-rules"; path = pkgs.writeText "fp6-misc-udev.rules" c.services.udev.extraRules; }
  { name = "generator-fp6-charging-mode"; path = c.systemd.generators.fp6-charging-mode; }
] ++ pkgs.lib.optional withMediaplayer
  { name = "mediaplayer"; path = c.system.build.fp6-mediaplayer-hwdecode; })
