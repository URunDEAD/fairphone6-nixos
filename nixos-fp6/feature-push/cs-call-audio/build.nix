# Standalone builds for the CS call audio feature (ARM builder):
#   nix-remote build --eval-store daemon --file nixos-fp6/feature-push/cs-call-audio/build.nix <attr>
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs
, pkgs ? import nixpkgs { system = "aarch64-linux"; }
}:
let
  # Same composition as the module: our patch on pkgs.ofono, then the
  # cellular package's own overrideAttrs (its 4 patches) on top.
  pkgsWithOfono = pkgs.extend (_final: prev: {
    ofono = prev.ofono.overrideAttrs (old: {
      patches = (old.patches or [ ]) ++ [ ./ofono-qrtrqmi-sw-call-volume.patch ];
    });
  });
  cellular = import ../cellular/packages.nix { pkgs = pkgsWithOfono; };
in
{
  csvoiced = pkgs.callPackage ./package.nix { };
  ofono = cellular.ofono;
  # Base graphs only: proves the patch leaves the running topology unchanged.
  topology = pkgs.callPackage ./topology/package.nix { };
  # The real voice graph (generated from the stock ACDB by topology/acdb2m4.py).
  topologyVoice = pkgs.callPackage ./topology/package.nix {
    voiceGraph = ./topology/milos-voice-graph.m4;
  };
  # The kernel6 target with the feature enabled in its default dry-run
  # mode: the generated unit, config and the oFono actually used by the
  # system, without building a second toplevel.
  dryRunCheck =
    let
      target = import ../trial-lomiri-full-kernel6-fp6.nix { inherit nixpkgs; };
      enabled = target.eval.extendModules {
        modules = [ { hardware.fairphone6.csCallAudio.enable = true; } ];
      };
      c = enabled.config;
      ofono = c.system.build.fp6-cellular.ofono;
    in
    pkgs.runCommand "fp6-cs-call-audio-dryrun-check" { } ''
      mkdir -p $out
      cp ${c.systemd.units."fp6-csvoiced.service".unit}/fp6-csvoiced.service $out/
      cp ${c.environment.etc."fp6-csvoiced.conf".source} $out/fp6-csvoiced.conf
      grep -qx 'dry_run = true' $out/fp6-csvoiced.conf
      grep -q 'qmimodem_sw' ${ofono}/bin/ofonod
      echo ${ofono} > $out/ofono-path
      echo ${c.system.build.fp6-csvoiced} > $out/csvoiced-path
    '';
}

