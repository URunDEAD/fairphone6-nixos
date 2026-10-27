# Always-on OTA update server for the release machine (the x86 NixOS
# laptop). Serves the release root written by `fp6-release publish` /
# ota-release.sh: cache/ (a plain Nix binary cache: nix-cache-info,
# *.narinfo, nar/*.nar.zst, every path signed with the release Nix key) and
# channels/<channel>/ (manifest.json + .minisig + closure list), as static
# files over HTTP with `fp6-release serve`. The phone copies with
# `nix copy --from http://<host>:8742/cache`, signatures enforced.
#
# Import it from the laptop's flake (NOT applied by the FP6 work itself):
#   imports = [ /home/<user>/Desktop/and/nixos-fp6/feature-push/ota/server/fp6-ota-server.nix ];
#   services.fp6OtaServer = {
#     enable = true;
#     publisher = "<user>";                      # owns /srv/fp6-ota, runs ota-release.sh
#     firewallInterfaces = [ "wt0" "wlp0s20f3" ]; # NetBird + Wi-Fi LAN
#   };
# then `sudo nixos-rebuild switch --flake .#<host>`.
#
# The service only reads the root (DynamicUser, ProtectSystem=strict); the
# publisher writes it. Nothing secret is served: the secret keys stay in
# results/feature-push-20261024/ota/keys and are never under the root.
{ config, lib, pkgs, ... }:
let
  cfg = config.services.fp6OtaServer;
  core = (pkgs.callPackage ../package.nix { }).core;
in {
  options.services.fp6OtaServer = {
    enable = lib.mkEnableOption "the FP6 OTA update server (fp6-release serve)";
    root = lib.mkOption {
      type = lib.types.path;
      default = "/srv/fp6-ota";
      description = "Release root (cache/ and channels/).";
    };
    port = lib.mkOption { type = lib.types.port; default = 8742; };
    listenAddresses = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "0.0.0.0" ];
      description = "Addresses to bind; reachability is limited by firewallInterfaces.";
    };
    publisher = lib.mkOption {
      type = lib.types.str;
      description = "Account that publishes releases (owns the root).";
    };
    firewallInterfaces = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "wt0" "wlp0s20f3" ];
      description = "Interfaces on which the port is opened (NetBird, LAN). Never opened globally.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.tmpfiles.rules = [ "d ${cfg.root} 0755 ${cfg.publisher} users -" ];

    systemd.services.fp6-ota-server = {
      description = "FP6 OTA update server (static binary cache + signed manifests)";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig = {
        ExecStart = "${core}/bin/fp6-release serve --root ${cfg.root} --listen "
          + lib.concatMapStringsSep "," (a: "${a}:${toString cfg.port}") cfg.listenAddresses;
        Restart = "always";
        RestartSec = 5;
        DynamicUser = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        ReadOnlyPaths = [ cfg.root ];
        PrivateTmp = true;
        PrivateDevices = true;
        NoNewPrivileges = true;
        CapabilityBoundingSet = "";
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
        RestrictNamespaces = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" ];
        Nice = 5;
        IOSchedulingClass = "idle";
      };
    };

    networking.firewall.interfaces = lib.genAttrs cfg.firewallInterfaces (_: { allowedTCPPorts = [ cfg.port ]; });
  };
}
