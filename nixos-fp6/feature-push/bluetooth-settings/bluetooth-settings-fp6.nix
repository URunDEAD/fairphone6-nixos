# Bluetooth settings (feature push 2026-10-24): visibility, pairing requests,
# phone name, paired devices with type/icon, rename, battery, last connected,
# per-profile connect, forget; paired devices in the status-bar menu.
# Notes, previews, store format and the phone test list:
# results/feature-push-20261024/bluetooth-settings/README.md
#
#  * System Settings > Bluetooth: the stock lomiri-system-settings 1.3.2
#    plugin, patched (lss-bluetooth-fp6.patch) and rebuilt out of tree; its
#    files replace the stock ones in the lomiri-system-settings wrapper
#    (overlay lib.mkOrder 2960, after wifi-qr 2950, before storage-backup
#    3000). Neither lomiri-system-settings-unwrapped nor Lomiri is rebuilt.
#  * Defaults (Android-like): visible and accepting pairing requests only
#    while the Bluetooth page is open and System Settings is in the
#    foreground; BlueZ safety timeouts (180 s, re-armed every minute) end
#    both if the app is frozen or dies. Without the page no pairing agent
#    is registered and BlueZ (AlwaysPairable=false) is not bondable.
#  * Status bar: ayatana-indicator-bluetooth with indicator-bluetooth-fp6.patch
#    (phone profile: paired devices as connect switches with type icon and
#    "Connected · battery N %"), replaced through the overlay (leaf package).
#  * Chosen name/type per device (car, speaker, earbuds, ...): per-user
#    $XDG_CONFIG_HOME/fp6-bluetooth/devices.ini, one [AA:BB:..] group with
#    name=/type= (the format the sound UI reads; 0600 in a 0700 dir), read by
#    the indicator too. Type list and icons: the results README.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.bluetoothSettings;
  boolStr = b: if b then "true" else "false";
  icons = (pkgs.callPackage ./package.nix { }).icons;
in
{
  options.hardware.fairphone6.bluetoothSettings = {
    enable = lib.mkEnableOption ''
      the FP6 Bluetooth settings: visibility and pairing-request switches,
      phone name, paired-device management (rename, type/icon, battery, last
      connected, profiles, forget) and paired devices in the Bluetooth menu'';

    visible = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''Default of "Visible to nearby devices" (users can change it in Settings).'';
    };
    visibilityTimeout = lib.mkOption {
      type = lib.types.enum [ 0 120 300 900 3600 ];
      default = 0;
      description = ''
        Default of "Stay visible": 0 = only while the Bluetooth page is
        open (recommended), otherwise seconds after opening the page.'';
    };
    allowPairing = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Default of "Allow pairing requests". Requests are only ever possible
        while the Bluetooth page is open; false refuses them there as well
        (pairing initiated from the phone still works).'';
    };
    indicator = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Paired devices with connect switches in the Bluetooth status-bar menu (patched indicator).";
    };
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = lib.mkOrder 2960 [ (_final: prev: let
      pkg = prev.callPackage ./package.nix { inherit (prev) ayatana-indicator-bluetooth lomiri; };
    in {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        lomiri-system-settings = lprev.lomiri-system-settings.overrideAttrs (old: {
          postInstall = (old.postInstall or "") + ''
            for sub in share/lomiri-system-settings/qml-plugins/bluetooth \
                       lib/lomiri-system-settings/private/Lomiri/SystemSettings/Bluetooth; do
              test -d "$out/$sub"
              for f in ${pkg.lssBluetooth}/$sub/*; do
                ln -sfn "$f" "$out/$sub/$(basename "$f")"
              done
            done
            grep -q fp6PhoneName $out/share/lomiri-system-settings/qml-plugins/bluetooth/PageComponent.qml
            test "$(readlink -f $out/lib/lomiri-system-settings/private/Lomiri/SystemSettings/Bluetooth/libLomiriBluetoothPanel.so)" \
              = "$(readlink -f ${pkg.lssBluetooth}/lib/lomiri-system-settings/private/Lomiri/SystemSettings/Bluetooth/libLomiriBluetoothPanel.so)"
          '';
          passthru = (old.passthru or { }) // { fp6BluetoothPanel = pkg.lssBluetooth; };
        });
      });
    } // lib.optionalAttrs cfg.indicator {
      ayatana-indicator-bluetooth = pkg.indicator;
    }) ];

    # System defaults; per-user choices go to ~/.config/fp6-bluetooth/settings.ini.
    environment.etc."fp6-bluetooth/settings.ini".text = ''
      [General]
      visible=${boolStr cfg.visible}
      visibility-timeout=${toString cfg.visibilityTimeout}
      allow-pairing=${boolStr cfg.allowPairing}
    '';

    # Earbuds icon (Suru has none); Suru inherits hicolor.
    environment.systemPackages = [ icons ];

    # Bonding only while a pairing agent (the Settings page) is registered.
    # BlueZ's default, stated so nothing turns it on by accident.
    hardware.bluetooth.settings.General.AlwaysPairable = lib.mkDefault false;

    system.build.fp6-bluetooth-settings = {
      inherit icons;
      inherit (pkgs.lomiri) lomiri-system-settings;
      panel = pkgs.lomiri.lomiri-system-settings.fp6BluetoothPanel;
      indicator = pkgs.ayatana-indicator-bluetooth;
    };
  };
}
