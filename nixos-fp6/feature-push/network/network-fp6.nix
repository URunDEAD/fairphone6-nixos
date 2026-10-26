# FP6 network feature push (2026-10-24) for the Lomiri target.
#
# Opt-in umbrella module:  imports = [ ./feature-push/network/network-fp6.nix ];
#                          hardware.fairphone6.network.enable = true;
#
# Covers Bluetooth (BlueZ + PipeWire A2DP/HFP), the NetworkManager Wi-Fi
# hotspot (shared mode) used by lomiri-indicator-network/system-settings,
# Lomiri flight mode (an org.freedesktop.URfkill provider, which nixpkgs
# lacks), an optional conservative neard for the Samsung S3NRN4V NFC chip and
# an optional Wi-Fi country hint. Hardware facts and rationale:
# results/feature-push-20261024/network/README.md.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.network;
  urfkill = pkgs.callPackage ./fp6-urfkill-package.nix { };
  flightTypes = lib.concatStringsSep "," cfg.flightMode.radioTypes;
in
{
  options.hardware.fairphone6.network = {
    enable = lib.mkEnableOption "FP6 network integration for the Lomiri session";

    bluetooth = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "BlueZ with PipeWire A2DP/HFP audio and the Lomiri/Ayatana Bluetooth UI.";
      };
      hfpBackend = lib.mkOption {
        type = lib.types.enum [ "native" "ofono" "none" ];
        default = "native";
        description = ''
          PipeWire HSP/HFP backend. "native" lets PipeWire act as HFP audio
          gateway for headsets (headset mic/speaker for VoIP and media).
          Routing *cellular* call audio to a headset additionally needs the
          modem voice path from the cellular/audio agents; "ofono" hands HFP to
          oFono's HFP plugin if that stack is chosen instead.
        '';
      };
    };

    hotspot = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = ''
          Allow DHCP/DNS from NetworkManager's shared-mode dnsmasq on the
          hotspot interface. Lomiri's hotspot (lomiri-indicator-network) runs
          the AP on the same Wi-Fi device and deliberately disconnects the
          client Wi-Fi first; it is meant to share mobile data.
        '';
      };
      interface = lib.mkOption {
        type = lib.types.str;
        default = "wlan0";
        description = "Interface NetworkManager uses for the AP (Lomiri picks the only Wi-Fi device).";
      };
    };

    flightMode = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
        description = "Provide org.freedesktop.URfkill so Lomiri's flight-mode switches work.";
      };
      radioTypes = lib.mkOption {
        type = lib.types.listOf (lib.types.enum [ "wlan" "bluetooth" "nfc" "wwan" "gps" "fm" "uwb" "wimax" ]);
        default = [ "wlan" "bluetooth" "nfc" "wwan" "gps" "fm" "uwb" "wimax" ];
        description = "rfkill types soft-blocked in flight mode (FP6 currently exposes wlan, bluetooth, nfc).";
      };
      modemBackend = lib.mkOption {
        type = lib.types.enum [ "none" "ofono" "modemmanager" ];
        default = "ofono";
        description = ''
          How flight mode takes the cellular modem offline. Default "ofono":
          the cellular agent chose oFono (QRTR plugin) as the sole modem
          manager on 2026-10-24. The FP6 modem
          (IPA/rmnet, no rfkill wwan switch) is owned by the cellular agent:
          "ofono" sets org.ofono.Modem Online=false (what Lomiri's own wwan
          code expects), "modemmanager" calls Modem.Enable(false). Only modems
          the daemon took offline are brought back when flight mode ends.
        '';
      };
    };

    nfc = {
      enable = lib.mkEnableOption ''
        neard for the kernel-NCI Samsung S3NRN4V (s3fwrn5_i2c, nfc0) with
        DefaultPowered/ConstantPoll/ResetOnError all false, so nothing powers
        the RF front end until an application asks. No Lomiri NFC UI exists in
        nixpkgs (the system-settings NFC panel needs Android properties and
        Sailfish nfcd); mutually exclusive with nfcMetadataTrial'';
    };

    wifiCountry = lib.mkOption {
      type = lib.types.nullOr (lib.types.strMatching "[A-Z]{2}");
      default = null;
      example = "RO";
      description = ''
        Optional ISO country hint passed to the self-managed ath11k
        regulatory domain after boot (iw reg set). The firmware currently
        boots as "US" after "failed to process regulatory info -22".
      '';
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      networking.networkmanager.enable = true;
      # Firmware is already provided by the device firmware package on the
      # current system: ath11k/WCN6750/hw1.0/{wpss.mdt,board-2.bin} and
      # qca/{msbtfw12.mbn,msnv12.bin}; no extra blobs are needed.
      environment.systemPackages = [ pkgs.iw ]; # rfkill comes from util-linux
    }

    (lib.mkIf cfg.bluetooth.enable {
      hardware.bluetooth = {
        enable = true;
        powerOnBoot = lib.mkDefault true;
        settings = {
          General = {
            # Battery level of headsets (org.bluez.Battery1) for the indicator.
            Experimental = lib.mkDefault true;
            JustWorksRepairing = lib.mkDefault "confirm";
          };
          Policy.ReconnectAttempts = lib.mkDefault 7;
        };
      };
      services.pipewire = {
        enable = lib.mkDefault true;
        pulse.enable = lib.mkDefault true;
        wireplumber.enable = lib.mkDefault true;
        wireplumber.extraConfig."51-fp6-bluez" = {
          "monitor.bluez.properties" = {
            "bluez5.roles" = [ "a2dp_sink" "a2dp_source" "bap_sink" "bap_source" ]
              ++ lib.optionals (cfg.bluetooth.hfpBackend != "none") [ "hsp_ag" "hfp_ag" ];
            "bluez5.hfphsp-backend" = cfg.bluetooth.hfpBackend;
            "bluez5.enable-sbc-xq" = true;
            "bluez5.enable-msbc" = true;
            "bluez5.enable-hw-volume" = true;
          };
        };
      };
      # pactl for the live test and for Lomiri's sound indicator diagnostics.
      environment.systemPackages = [ pkgs.pulseaudio ];
    })

    (lib.mkIf cfg.hotspot.enable {
      # NetworkManager's shared mode spawns its own dnsmasq bound to the AP
      # interface (the system dnsmasq only serves usb0 with SO_BINDTODEVICE)
      # and installs its own nftables NAT table; the NixOS input chain must
      # still admit DHCP and DNS from hotspot clients. IP forwarding is
      # enabled by NetworkManager when the shared connection comes up.
      networking.firewall.interfaces.${cfg.hotspot.interface} = {
        allowedUDPPorts = [ 53 67 ];
        allowedTCPPorts = [ 53 ];
      };
    })

    (lib.mkIf cfg.flightMode.enable {
      services.dbus.packages = [ urfkill ];
      environment.systemPackages = [ urfkill ];
      systemd.services.fp6-urfkill = {
        description = "FP6 URfkill-compatible flight-mode service";
        wantedBy = [ "multi-user.target" ];
        after = [ "dbus.service" "systemd-rfkill.service" ];
        before = [ "display-manager.service" ];
        serviceConfig = {
          Type = "dbus";
          BusName = "org.freedesktop.URfkill";
          ExecStart = "${lib.getExe urfkill} --flight-types ${flightTypes} --modem-backend ${cfg.flightMode.modemBackend}";
          StateDirectory = "fp6-urfkill";
          Restart = "on-failure";
          DeviceAllow = [ "/dev/rfkill rw" ];
          DevicePolicy = "closed";
          ProtectSystem = "strict";
          ProtectHome = true;
          PrivateTmp = true;
          NoNewPrivileges = true;
          CapabilityBoundingSet = [ "" ];
          RestrictAddressFamilies = [ "AF_UNIX" ];
        };
      };
      # systemd-rfkill restores per-device soft state at boot; fp6-urfkill
      # then re-applies a persisted flight mode on top of it.
    })

    (lib.mkIf cfg.nfc.enable {
      assertions = [
        { assertion = !config.hardware.nfc-nci.enable;
          message = "FP6 NFC is Samsung kernel NCI (s3fwrn5); the NXP PN5xx nfc-nci module must stay disabled."; }
        { assertion = !(config.hardware.fairphone6.nfcMetadataTrial.enable or false);
          message = "nfcMetadataTrial requires the global neard to be off; disable one of them."; }
      ];
      services.neard = {
        enable = true;
        settings.General = {
          DefaultPowered = false;
          ConstantPoll = false;
          ResetOnError = false;
        };
      };
    })

    (lib.mkIf (cfg.wifiCountry != null) {
      systemd.services.fp6-wifi-country = {
        description = "Set Wi-Fi regulatory country hint";
        wantedBy = [ "multi-user.target" ];
        after = [ "sys-subsystem-net-devices-wlan0.device" ];
        bindsTo = [ "sys-subsystem-net-devices-wlan0.device" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "${lib.getExe pkgs.iw} reg set ${cfg.wifiCountry}";
        };
      };
    })
  ]);
}
