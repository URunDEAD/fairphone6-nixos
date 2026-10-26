# USB gadget network repair: keep usb0 at exactly 172.16.42.1/24 so the
# system dnsmasq (configuration.nix) can lease 172.16.42.2 again.
#
# Evidence (phone, boot 2026-10-24 16:29, read-only):
#  * stage-1 (mobile-nixos dhcpd-task.rb) runs `ifconfig usb0 172.16.42.1`,
#    which takes the classful mask: 172.16.42.1/16.
#  * network-addresses-usb0 adds 172.16.42.1/24 at 16:29:18, but one second
#    later udev re-applies 40-usb0.link (gadget re-add while adbd/NM start);
#    the unit is BindsTo= the device, so systemd stops it and its pre-stop
#    deletes the /24. It is not started again. Only the stage-1 /16 remains.
#  * dnsmasq's range carries an explicit 255.255.255.0 mask that no longer
#    matches the interface -> "DHCPDISCOVER(usb0) ... no address available".
#
# Fix (both parts are independent; either alone restores DHCP):
#  1. fp6-usb0-address: a small watcher that normalizes usb0 to exactly
#     172.16.42.1/24 at start and after every address/link change on usb0
#     (adds the /24 first, then removes any other global IPv4 address, so an
#     SSH session over 172.16.42.1 is never left without the address).
#  2. dnsmasq's dhcp-range drops the explicit netmask, so dnsmasq derives it
#     from the interface address and keeps serving even if the prefix drifts.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.usbNet;
  want = "${cfg.address}/${toString cfg.prefixLength}";
  normalize = pkgs.writeShellApplication {
    name = "fp6-usb0-address";
    runtimeInputs = [ pkgs.iproute2 pkgs.coreutils pkgs.gawk ];
    text = ''
      dev=${lib.escapeShellArg cfg.interface}
      want=${lib.escapeShellArg want}
      current() {
        ip -4 -o addr show dev "$dev" scope global 2>/dev/null | awk '{print $4}' | sort
      }
      normalize() {
        ip link show dev "$dev" >/dev/null 2>&1 || return 0
        # Only act when the set differs; a no-op replace would emit another
        # RTM_NEWADDR and re-trigger the monitor loop.
        [ "$(current)" = "$want" ] && return 0
        ip link set dev "$dev" up || true
        ip addr replace "$want" dev "$dev" nodad || return 0
        for a in $(current); do
          [ "$a" = "$want" ] || ip addr del "$a" dev "$dev" || true
        done
        # Deleting a former primary without promote_secondaries also drops
        # the /24 secondary; make sure it is present afterwards.
        if ! current | grep -qx "$want"; then
          ip addr replace "$want" dev "$dev" nodad || true
        fi
        echo "fp6-usb0-address: $dev now $(current | tr '\n' ' ')"
      }
      normalize
      ip -o monitor address link | while read -r line; do
        case "$line" in
          *" $dev "*|*" $dev:"*) sleep 0.2; normalize ;;
        esac
      done
    '';
  };
in {
  options.hardware.fairphone6.usbNet = {
    enable = lib.mkEnableOption "the usb0 /24 address watcher and prefix-agnostic dnsmasq range";
    interface = lib.mkOption { type = lib.types.str; default = "usb0"; };
    address = lib.mkOption { type = lib.types.str; default = "172.16.42.1"; };
    prefixLength = lib.mkOption { type = lib.types.int; default = 24; };
    hostAddress = lib.mkOption { type = lib.types.str; default = "172.16.42.2"; };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.fp6-usb0-address = {
      description = "Keep ${cfg.interface} at exactly ${want} for the USB DHCP server";
      wantedBy = [ "multi-user.target" ];
      before = [ "dnsmasq.service" ];
      after = [ "network-pre.target" "network-addresses-${cfg.interface}.service" ];
      # Deliberately no BindsTo= on the device: that is what lost the /24.
      serviceConfig = {
        ExecStart = "${normalize}/bin/fp6-usb0-address";
        Restart = "always";
        RestartSec = 2;
        CapabilityBoundingSet = [ "CAP_NET_ADMIN" ];
        AmbientCapabilities = [ "CAP_NET_ADMIN" ];
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
      };
    };
    services.dnsmasq.settings.dhcp-range = lib.mkForce [
      "${cfg.hostAddress},${cfg.hostAddress},12h"
    ];
    system.build.fp6-usb0-address = normalize;
    assertions = [{
      assertion = builtins.elem { address = cfg.address; prefixLength = cfg.prefixLength; }
          config.networking.interfaces.${cfg.interface}.ipv4.addresses
        && config.services.dnsmasq.enable;
      message = "fp6 usbNet: ${want} must match networking.interfaces.${cfg.interface} and dnsmasq must be on.";
    }];
  };
}
