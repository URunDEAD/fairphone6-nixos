# NetBird (WireGuard mesh VPN) on the FP6, switched from Lomiri's network menu.
#
#  * Daemon: nixpkgs services.netbird client "netbird" -> netbird.service,
#    state /var/lib/netbird, socket /var/run/netbird/sock, interface wt0.
#    Hardened (runs as the netbird system user with CAP_NET_ADMIN/RAW/BPF; DNS
#    through systemd-resolved via the module's polkit rule). autoStart = false:
#    the daemon runs but never connects by itself.
#  * Control without root: the socket directory is 0750 netbird:netbird and the
#    phone user is in the netbird group (upstream's mechanism for hardened
#    clients), so `netbird login/up/down/profile ...` work as that user.
#  * Lomiri: lomiri-indicator-network lists every NM connection of type "vpn"
#    with an on/off switch. A NetworkManager VPN plugin
#    (org.freedesktop.NetworkManager.netbird, nm-netbird-service.py) plus a
#    declarative NM connection "NetBird" make that switch run
#    `netbird up` / `netbird down` for the daemon's active profile.
#    The plugin hands NM no IP config: NetBird keeps managing wt0, its routes
#    and DNS; WiFi / rmnet routes and DNS are untouched.
#  * Default: not logged in, disconnected. The user logs in once on the CLI
#    (see results/feature-push-20261024/netbird/README.md).
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.netbird;
  client = config.services.netbird.clients.netbird;
  fp6 = import ./packages.nix { inherit pkgs; netbird = client.wrapper; };
in {
  options.hardware.fairphone6.netbird = {
    enable = lib.mkEnableOption "NetBird with a Lomiri/NetworkManager VPN switch";
    user = lib.mkOption {
      type = lib.types.str;
      default = "user";
      description = "Phone user allowed to control the NetBird daemon (netbird group).";
    };
    connectionName = lib.mkOption {
      type = lib.types.str;
      default = "NetBird";
      description = "Name of the NetworkManager VPN connection shown in the network menu.";
    };
  };

  config = lib.mkIf cfg.enable {
    services.netbird = {
      ui.enable = false;
      clients.netbird = {
        port = 51820;
        interface = "wt0";
        hardened = true;
        # Never connect at daemon start; NM / the user decide.
        autoStart = false;
      };
    };

    users.users.${cfg.user}.extraGroups = [ client.user.group ];

    networking.networkmanager.plugins = [ fp6.networkmanager-netbird ];

    # Runtime-only keyfile (/run/NetworkManager/system-connections), recreated
    # at every boot; nothing persistent is written.
    networking.networkmanager.ensureProfiles.profiles.fp6-netbird = {
      connection = {
        id = cfg.connectionName;
        uuid = "6e3c1f0a-9b7d-4c55-8f0e-0b1d5e7a4b21";
        type = "vpn";
        # NM never autoconnects VPN profiles anyway; reconnecting after a
        # restart is fp6-vpn-restore's job (persistence/persistence-fp6.nix).
        autoconnect = "false";
      };
      vpn = {
        service-type = "org.freedesktop.NetworkManager.netbird";
        # Stay up across WiFi <-> mobile data changes (NetBird handles roaming).
        persistent = "true";
        # Same budget as the plugin's `netbird up` (management login + first sync).
        timeout = "90";
      };
      ipv4 = {
        method = "auto";
        never-default = "true";
        ignore-auto-dns = "true";
        auto-route-ext-gw = "0"; # NM ternary: 0 = no route to the (placeholder) gateway
      };
      ipv6 = {
        method = "auto";
        never-default = "true";
        ignore-auto-dns = "true";
        auto-route-ext-gw = "0"; # NM ternary: 0 = no route to the (placeholder) gateway
      };
    };

    system.build.fp6-netbird = {
      inherit (fp6) networkmanager-netbird;
      netbird = client.wrapper;
    };

    assertions = [
      {
        assertion = config.networking.networkmanager.enable;
        message = "hardware.fairphone6.netbird needs NetworkManager.";
      }
      {
        assertion = config.services.resolved.enable;
        message = "The hardened NetBird client needs systemd-resolved for its DNS.";
      }
    ];
  };
}
