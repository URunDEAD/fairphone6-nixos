# State across reboots (feature push 2026-10-24, Android-like).
#
# Audit of every user-facing toggle (where it is stored, whether it survives
# a restart): results/feature-push-20261024/persistence/README.md.
# Everything except the VPN switch already persisted (NetworkManager.state,
# systemd-rfkill, fp6-urfkill, oFono, dconf, ~/.config, /var/lib/*).
#
# VPN: NetworkManager never autoconnects VPN profiles (nm-settings-nmcli(5):
# "autoconnect is not implemented for VPN profiles"), so the network menu's
# NetBird switch was off after every boot (and after every OTA install, which
# is why the OTA health check reported "vpn-connected: fail").
#  * fp6-vpn-restore (Rust, user session agent): follows NM's VPN active
#    connections, keeps the user's last choice per VPN (dconf
#    com.fairphone6.vpn last-active; only a user switch-off clears it, drops
#    and shutdown do not) and activates them again once a network is up,
#    with back-off.
#  * Settings > VPN > Reconnect at startup, per VPN: Restore last state
#    (default), Always on (connect at start, reconnect when it drops), Off.
#  * Any NM VPN profile, not only NetBird; WireGuard profiles (type
#    "wireguard") are device connections that NM autoconnects itself.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.persistence;
  fp6 = pkgs.callPackage ./package.nix { };
in {
  options.hardware.fairphone6.persistence = {
    enable = lib.mkEnableOption "restoring state across reboots (VPN reconnect at startup)";
    user = lib.mkOption {
      type = lib.types.str;
      default = config.services.fp6LomiriSensors.user or "user";
      description = "Phone user whose session runs the VPN restore agent.";
    };
    vpnRestore = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Reconnect NetworkManager VPNs after a restart (per-VPN choice in
        Settings > VPN; default: restore the last state).'';
    };
  };

  config = lib.mkIf (cfg.enable && cfg.vpnRestore) {
    assertions = [{
      assertion = config.networking.networkmanager.enable;
      message = "hardware.fairphone6.persistence.vpnRestore needs NetworkManager.";
    }];

    systemd.user.services.fp6-vpn-restore = {
      description = "FP6 VPN reconnect at startup (restore last state / always-on)";
      wantedBy = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      after = [ "graphical-session.target" ];
      unitConfig.ConditionUser = cfg.user;
      serviceConfig = {
        ExecStart = lib.getExe fp6.vpnRestore;
        Restart = "on-failure";
        RestartSec = 10;
        Nice = 5;
      };
    };

    # For gsettings/dconf on the CLI (where the profile links gsettings-schemas).
    environment.systemPackages = [ fp6.schema ];

    # Settings > VPN: the stock page plus the Reconnect-at-startup section.
    nixpkgs.overlays = lib.mkOrder 3400 [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri-system-settings = (lprev.lomiri-system-settings.override (old: {
          plugins = (old.plugins or [ ]) ++ [ fp6.lssPlugin ];
        })).overrideAttrs (old: {
          nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ pkgs.buildPackages.patch ];
          # Replace the page link by a patched copy; fails if upstream changed it.
          postInstall = (old.postInstall or "") + ''
            vpn=$out/share/lomiri-system-settings/qml-plugins/vpn
            src=$(readlink -f "$vpn/PageComponent.qml")
            cp --remove-destination "$src" "$vpn/PageComponent.qml"
            chmod u+w "$vpn/PageComponent.qml"
            patch -d "$vpn" -p1 --forward --batch < ${./lss-vpn-startup.patch}
            grep -q Fp6VpnStartup "$vpn/PageComponent.qml"
            test -e "$vpn/Fp6VpnStartup.qml"
          '';
          passthru = (old.passthru or { }) // { fp6VpnStartup = true; };
        });
      });
    }) ];

    system.build.fp6-persistence = { inherit (fp6) vpnRestore schema lssPlugin; inherit (pkgs.lomiri) lomiri-system-settings; };
  };
}
