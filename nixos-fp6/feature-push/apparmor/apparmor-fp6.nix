# AppArmor confinement for network-facing / untrusted-content apps on the FP6
# (Lomiri on NixOS). Defensive hardening only: per-binary profiles attached by
# Nix store path, loaded by the stock NixOS apparmor.service.
#
#  * Profile name == the app's Lomiri application id (desktop file id), so the
#    AppArmor label that content-hub reads from D-Bus (LinuxSecurityLabel)
#    matches the APP_ID the app supplies; with any other name content-hub
#    would reject the app's imports/exports ("Supplied app ID doesn't match
#    AppArmor profile").
#  * Attachment by store-path glob (/nix/store/*-<pname>-*/bin/...), so a
#    rebuilt or re-wrapped package keeps its profile without listing every
#    wrapper layer (shell wrapper -> makeWrapper ELF -> .<name>-wrapped).
#  * Read + mmap of the whole (root-owned, read-only) /nix/store is allowed in
#    abstractions/fp6-nixos-base; the confinement is on writes, $HOME, devices,
#    sockets, signals/ptrace and exec. See
#    results/feature-push-20261024/apparmor/README.md.
#
# Not used: lomiri-app-launch's own legacy-app confinement
# (X-Lomiri-AppArmor-Profile= in the desktop file -> "aa-exec -p <profile>"):
# nixpkgs builds it with AA_EXEC_PATH=/usr/bin/aa-exec (absent on NixOS), it
# only applies to launches through the Lomiri drawer (not url-dispatcher or
# content-hub activations started elsewhere), and it also rewrites TMPDIR and
# XDG_DATA_DIRS for the app. Path attachment gives the same label everywhere.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.apparmor;

  abstractions = [
    "fp6-nixos-base"
    "fp6-gui"
    "fp6-audio"
    "fp6-network-client"
    "fp6-user-media-read"
    "fp6-user-downloads"
  ];

  # Default mode per profile (see the README for the test record).
  apps = {
    morph-browser = {
      default = "enforce";
      description = "Morph Browser + its QtWebEngine processes (which run with --no-sandbox).";
    };
    firefox = {
      default = "complain";
      description = "Firefox (large; complain until it has had more real-world use).";
    };
    "de.haeckerfelix.Fragments" = {
      default = "enforce";
      description = "Fragments BitTorrent client and its transmission-daemon.";
    };
    lomiri-mediaplayer-app = {
      default = "enforce";
      description = "Lomiri Media Player (GStreamer + V4L2 hardware decode).";
    };
    "io.github.rinigus.PureMaps" = {
      default = "enforce";
      description = "Pure Maps (+ pure-maps-uri-handler, same binary).";
    };
    lomiri-messaging-app = {
      default = "enforce";
      description = "Lomiri Messaging (SMS/MMS UI).";
    };
    lomiri-camera-app = {
      default = "complain";
      description = "Lomiri Camera / barcode reader (not exercised live yet).";
    };
    nuntium = {
      default = "complain";
      description = "nuntium MMS daemon (parses MMS PDUs from the network).";
    };
  };
in {
  options.hardware.fairphone6.apparmor = {
    enable = lib.mkEnableOption "the FP6 AppArmor profiles for Lomiri apps and MMS";
    apps = lib.mapAttrs (name: a: lib.mkOption {
      type = lib.types.enum [ "disable" "complain" "enforce" ];
      default = a.default;
      description = "AppArmor mode for the `${name}` profile: ${a.description}";
    }) apps;
  };

  config = lib.mkIf cfg.enable {
    security.apparmor.enable = true;

    security.apparmor.includes = lib.listToAttrs (map (n:
      lib.nameValuePair "abstractions/${n}" (builtins.readFile (./abstractions + "/${n}"))
    ) abstractions);

    security.apparmor.policies = lib.mapAttrs (name: state: {
      inherit state;
      path = ./profiles + "/${name}";
    }) cfg.apps;

    # Denials since boot, one line per distinct (profile, operation, path).
    environment.systemPackages = [
      (pkgs.writeShellScriptBin "fp6-apparmor-denials" ''
        # Usage: fp6-apparmor-denials [journalctl --since argument]
        exec ${config.systemd.package}/bin/journalctl -b ''${1:+--since "$1"} -o cat --no-pager -g 'apparmor="(DENIED|ALLOWED)"' \
          | ${pkgs.gnused}/bin/sed -E 's/.*apparmor="([A-Z]+)" operation="([^"]*)".*profile="([^"]*)".*name="([^"]*)".*requested_mask="([^"]*)".*/\1 \3 \2 \4 \5/' \
          | ${pkgs.coreutils}/bin/sort | ${pkgs.coreutils}/bin/uniq -c | ${pkgs.coreutils}/bin/sort -rn
      '')
    ];

    system.build.fp6-apparmor-policies = config.environment.etc."apparmor.d".source;

    # Build-time compile of every enabled FP6 profile against the generated
    # /etc/apparmor.d (NixOS abstraction overrides + ours) and the
    # apparmor-profiles package, as apparmor.service will at boot.
    system.checks = [
      (pkgs.runCommand "fp6-apparmor-profiles-compile" { } ''
        set -eu
        ${lib.concatStrings (lib.mapAttrsToList (name: state:
          lib.optionalString (state != "disable") ''
            echo "compile ${name} (${state})"
            ${pkgs.apparmor-parser}/bin/apparmor_parser --config-file /dev/null \
              -Q -K -I ${config.environment.etc."apparmor.d".source} \
              ${lib.concatMapStringsSep " " (p: "-I ${p}/etc/apparmor.d") config.security.apparmor.packages} \
              ${./profiles + "/${name}"} 2>&1 | { grep -v 'Cache read/write disabled' || true; }
            test "''${PIPESTATUS[0]}" = 0
          '') cfg.apps)}
        touch $out
      '')
    ];
  };
}
