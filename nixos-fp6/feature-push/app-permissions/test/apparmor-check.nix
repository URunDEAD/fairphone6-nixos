# Laptop (x86) version of the module's system.checks: every FP6 AppArmor
# profile compiles with the largest includes fp6-permd generates (all
# granted / all denied), against a NixOS /etc/apparmor.d with the FP6
# abstractions. The includes come from the x86 build of fp6-permd.
#   nix-build test/apparmor-check.nix
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  lib = pkgs.lib;
  core = (pkgs.callPackage ../package.nix { }).core;
  eval = import (pkgs.path + "/nixos/lib/eval-config.nix") {
    system = pkgs.stdenv.hostPlatform.system;
    modules = [ ../../apparmor/apparmor-fp6.nix {
      system.stateVersion = "26.05";
      boot.loader.grub.enable = false;
      fileSystems."/" = { device = "none"; fsType = "tmpfs"; };
      hardware.fairphone6.apparmor.enable = true;
    } ];
  };
  c = eval.config;
  profiles = lib.filterAttrs (n: p: p.state != "disable" && c.hardware.fairphone6.apparmor.apps ? ${n}) c.security.apparmor.policies;
in pkgs.runCommand "fp6-app-permissions-apparmor-check" { } ''
  set -eu
  for kind in allow deny; do
    inc=$TMPDIR/inc-$kind
    mkdir -p $inc/local
    ${lib.concatStrings (lib.mapAttrsToList (n: _: "cp ${core}/share/fp6-permissions/sample-$kind.inc $inc/local/${n}\n") profiles)}
    ${lib.concatStrings (lib.mapAttrsToList (n: p: ''
      echo "compile ${n} with the $kind include"
      ${pkgs.apparmor-parser}/bin/apparmor_parser --config-file /dev/null -Q -K \
        -I $inc -I ${c.environment.etc."apparmor.d".source} \
        ${lib.concatMapStringsSep " " (x: "-I ${x}/etc/apparmor.d") c.security.apparmor.packages} \
        ${p.path}
    '') profiles)}
  done
  echo "${toString (lib.attrNames profiles)}" > $out
''
