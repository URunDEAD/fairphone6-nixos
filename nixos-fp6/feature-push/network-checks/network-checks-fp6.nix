# Fixes from the 2026-10-26 live network checks (flight mode, hotspot, MTP,
# ADB) on the Lomiri/kernel6 system. Evidence and test steps:
# results/feature-push-20261024/network-checks/README.md.
#
# Opt-in:  imports = [ ./network-checks/network-checks-fp6.nix ];
#          hardware.fairphone6.networkChecks.enable = true;
#
# Hotspot (Settings > Hotspot, com.lomiri.connectivity1 SetHotspotEnabled):
#  * lomiri-indicator-network creates an AP connection without band/channel.
#    NetworkManager then picks a frequency from {2412, 2437, 2462, 2472} by
#    SSID hash ("Lomiri" -> 2472, channel 13). The self-managed ath11k regdomain
#    disables channels 12-13 (NM's cached channel list does not know), so
#    wpa_supplicant fails with "channel is disabled" and the hotspot never comes
#    up. The patched indicator pins band bg + a fixed channel (default 6) on
#    every add/update of its AP connection. Only the indicator service binary
#    is swapped (user-unit drop-in); the QML plugin used by the apps is
#    unchanged, so nothing else rebuilds.
#  * The cellular uplink (rmnet_data*) never gets IPv4 forwarding because
#    the oFono modem's IP is configured outside NM, so hotspot clients get no
#    replies. An NM dispatcher hook mirrors NM's "auto" forwarding on
#    rmnet_data* while a shared connection is active.
#  * After the hotspot is turned off, the Wi-Fi client stays disconnected (the
#    indicator's deactivation leaves it "blocked by user request"). The same
#    hook brings the newest autoconnect client connection back up.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.networkChecks;
  indicatorNetwork = pkgs.lomiri.lomiri-indicator-network.overrideAttrs (old: {
    pname = "${old.pname}-fp6-hotspot";
    patches = (old.patches or [ ]) ++ [ ./hotspot-ap-channel.patch ];
    # TestConnectivityApi.{,Insecure}HotspotConfig compare the exact AP
    # settings dict against the upstream one (no band/channel), so they fail
    # by design with this patch. Everything else is unchanged.
    doCheck = false;
    env = (old.env or { }) // {
      NIX_CFLAGS_COMPILE = toString [
        (old.env.NIX_CFLAGS_COMPILE or "")
        "-DFP6_HOTSPOT_CHANNEL=${toString cfg.hotspotChannel}"
      ];
    };
  });
  dispatcher = pkgs.replaceVarsWith {
    src = ./fp6-hotspot-dispatcher.sh;
    isExecutable = true;
    replacements.PATH = lib.makeBinPath [
      pkgs.networkmanager pkgs.coreutils pkgs.gnused pkgs.gawk
    ];
  };
in
{
  options.hardware.fairphone6.networkChecks = {
    enable = lib.mkEnableOption "FP6 hotspot fixes from the 2026-10-26 network checks";
    hotspotChannel = lib.mkOption {
      type = lib.types.ints.between 1 11;
      default = 6;
      description = ''
        2.4 GHz channel for the Lomiri hotspot. Channels 1-11 are allowed in
        every regulatory domain; ath11k disables 12-13 here.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.lomiri-indicator-network = {
      overrideStrategy = "asDropin";
      serviceConfig.ExecStart = [
        ""
        "${indicatorNetwork}/libexec/lomiri-indicator-network/lomiri-indicator-network-service"
      ];
    };
    system.build.fp6-indicator-network-hotspot = indicatorNetwork;

    networking.networkmanager.dispatcherScripts = [{
      type = "basic";
      source = dispatcher;
    }];
  };
}
