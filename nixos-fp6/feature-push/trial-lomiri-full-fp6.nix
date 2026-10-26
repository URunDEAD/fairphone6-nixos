# Accepted Lomiri GPU target (lomiri/trial-lomiri-gpu-fp6.nix, kernel4) plus every
# 2026-10-24 feature-push module: sound, sensors, camera, network, cellular, misc.
# Build/eval only; activation goes through tools/switch-phone-lomiri-gpu.py-style
# TEST activation with rollback. Never writes a boot image, slot or partition.
{ nixpkgs ? (import ../upstream/mobile-nixos/npins).nixpkgs
# Forwarded kernel pin; {} = kernel4. See trial-lomiri-full-kernel5-fp6.nix.
, kernelPin ? {}
}:
let
  base = import ./lomiri/trial-lomiri-gpu-fp6.nix { inherit nixpkgs kernelPin; };
  evaluated = base.eval.extendModules {
    modules = [
      ./audio/audio-fp6-feature.nix
      ./sensors/sensors-lomiri.nix
      ./camera/lomiri-camera-fp6.nix
      ./network/network-fp6.nix
      ./cellular/cellular-fp6.nix
      ./misc/rtc-offset-fp6.nix
      ./misc/sd-automount-fp6.nix
      ./misc/power-suspend-policy-fp6.nix
      ./misc/factory-reset-fp6.nix
      ./misc/mediaplayer-hwdecode-fp6.nix
      ./usb-net-fp6.nix
      ./lomiri/morph-browser-dpi-fp6.nix
      ./lomiri/firefox-lomiri-fp6.nix
      ./lomiri/gtk-lomiri-fp6.nix
      ./lomiri-polish/lomiri-polish-fp6.nix
      ./lomiri-misc/lomiri-misc-fp6.nix
      ./osk-wayland/osk-wayland-fp6.nix
      ./settings/settings-fp6.nix
      ./fingerprint/fingerprint-persist-fp6.nix
      ./fingerprint/biometryd-fprintd-fp6.nix
      ./boot-splash/boot-splash-fp6.nix
      ./misc/quick-checks-fp6.nix
      ./misc/haptics-fp6.nix
      ./misc/waydroid-lomiri-fp6.nix
      ./apps/fragments-fp6.nix
      ./apps/maps-fp6.nix
      ./netbird/netbird-fp6.nix
      ({ lib, options, ... }: {
        # fp6-lomiri-powerd replaces the manual-only brightness bridge.
        # lomiri-gpu-fp6.nix restates it at mkOverride 10.
        services.fp6ManualBrightness.enable = lib.mkOverride 5 false;
        services.fp6LomiriSensors.enable = true;
        # lomiri-gpu-fp6.nix already applies the qtmir orientation patch.
        services.fp6LomiriSensors.qtmirOrientationFix = false;
        hardware.fp6LomiriCamera.enable = true;
        # One camera app: Lomiri Camera. Snapshot needs the xdg camera portal (GNOME/Phosh only).
        hardware.fp6Camera.enableSnapshot = lib.mkForce false;
        hardware.fairphone6.network.enable = true;
        services.fp6Cellular.enable = true;
        services.fp6RtcOffset.enable = true;
        services.fp6SdAutomount.enable = true;
        # Guarded mode: no suspend until UFS hibern8 resume failures are fixed.
        services.fp6SuspendPolicy.enable = true;
        # hardware-fp6.nix keeps Wi-Fi powersave off for the SSH management path.
        services.fp6SuspendPolicy.wifiPowersave = false;
        # Installs the request/reset units only; nothing runs without an explicit request.
        services.fp6FactoryReset.enable = true;
        services.fp6MediaplayerHwDecode.enable = true;
        # usb0 /24 watcher + prefix-agnostic dnsmasq range (DHCP repair).
        hardware.fairphone6.usbNet.enable = true;
        # Qt6 Morph without QT_WAYLAND_FORCE_DPI (unverified squeeze fix).
        hardware.fairphone6.lomiriGpu.morphDpiFix.enable = true;
        # Mobile Firefox back, wrapped for Wayland/Mir (unverified).
        hardware.fairphone6.lomiriFirefox.enable = true;
        # GTK apps as phone apps: no window buttons, maximized staged windows, aspect-correct spread cards.
        hardware.fairphone6.lomiriGtk.enable = true;
        # Landscape OSK sizing, session menu (Log Out) on phone, QtSensors re-claim.
        hardware.fairphone6.lomiriPolish.enable = true;
        # Real launcher pins + greeter keyboard start condition.
        hardware.fairphone6.lomiriMisc.enable = true;
        # Maliit for Wayland text-input clients (Qt6 Morph, GTK3 Firefox) next to
        # the D-Bus path of the Qt5 apps. See osk-wayland/osk-wayland-fp6.nix.
        hardware.fairphone6.oskWayland.enable = true;
        # Battery, Brightness & Display and About pages in System Settings.
        hardware.fairphone6.settingsPanels.enable = true;
        # Writable fingerprint storage (user-approved). Manual start only until the
        # live backup/enroll procedure in results/feature-push-20261024/fingerprint passes.
        services.fp6FingerprintPersist = {
          enable = true;
          # Enrolment checked live 2026-10-26: start at boot (backup + rw persist first).
          startAtBoot = true;
          # Unlock goes through the Lomiri greeter (biometryd identify) only;
          # fingerprintd's own logind unlock and its LockedHint bridge would be
          # a second reader competing for the sensor.
          standbyUnlock = false;
          lomiriLockedHint = false;
        };
        # biometryd with the fprintd plugin: Settings > Security & Privacy > Lock
        # security > Fingerprint ID (add/list/delete) and greeter fingerprint unlock.
        services.fp6BiometrydFprintd.enable = true;
        # Power key -> screen off/lock (Unity.Screen setScreenPowerMode); live-tested via drop-in.
        services.fp6LomiriSensors.screenPower = true;
        # Quiet boot + Plymouth spinner; no getty/text between greeter and session.
        # hardware.fairphone6.bootSplash.verbose = true brings the debug boot text back.
        hardware.fairphone6.bootSplash.enable = true;
        # Offline-charging target, on-demand MTP, neard (see results/feature-push-20261024/quick-checks/).
        hardware.fairphone6.quickChecks.enable = true;
        # English UI, Romanian date/time format (dd.mm.yyyy, 24 h).
        i18n.supportedLocales = [ "en_US.UTF-8/UTF-8" "ro_RO.UTF-8/UTF-8" ];
        i18n.extraLocaleSettings.LC_TIME = "ro_RO.UTF-8";
        # Waydroid: 1116x2412 at 480 dpi (props are in scaled units), no auto-freeze.
        hardware.fairphone6.waydroidLomiri.enable = true;
        hardware.fairphone6.fragments.enable = true;
        # Pure Maps (UUITK flavour, keyless OSM providers), see apps/maps-fp6.nix.
        hardware.fairphone6.maps.enable = true;
        # NetBird daemon (idle, not logged in) + NM VPN plugin: on/off from the
        # VPN switch in the network menu. See netbird/netbird-fp6.nix.
        hardware.fairphone6.netbird.enable = true;
        system.nixos.tags = [ "full-features" ];
      })
    ];
  };
in base // {
  eval = evaluated;
  inherit (evaluated) config pkgs options;
  outputs = evaluated.config.mobile.outputs
    // evaluated.config.mobile.outputs.${evaluated.config.mobile.system.type};
}
