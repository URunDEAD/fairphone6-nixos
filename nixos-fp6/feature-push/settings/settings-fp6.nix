# FP6 lomiri-system-settings panels: Battery, Brightness, System information.
#
# Why they are missing on the stock nixpkgs build (lomiri-system-settings 1.3.2,
# identical on upstream main):
#  * Brightness and About ("System information") are only add_subdirectory()'d
#    when pkg-config finds libandroid-properties (libhybris). nixpkgs has no
#    such package, so neither panel is built. They only use property_get() for
#    cosmetic strings; android-properties-shim.h provides a header-only
#    property_get() that reads /etc/lomiri-system-settings/properties.
#  * Battery is built, but battery.settings carries
#    "visible-if-file-exists": "/etc/dbus-1/system.d/com.lomiri.Repowerd.conf"
#    (rewritten by nixpkgs to /run/current-system/sw/etc/...). fp6-lomiri-powerd
#    ships its policy as fp6-lomiri-powerd.conf through services.dbus.packages,
#    so the file never exists and the entry is always hidden. The line is
#    dropped; the panel's own dynamic visibility (UPower device count > 0)
#    stays in force.
#
# Double-tap-to-wake is not part of Ubuntu Touch's Brightness page; UT (and this
# build) show it as "Tap to wake" on the Gestures page, driven by
# com.canonical.Unity.Screen.getDoubleTapToWakeSupported and the
# com.lomiri.touch.system double-tap-to-wake key forwarded by Lomiri's Powerd
# plugin to setDoubleTapToWakeEnabled. Nothing extra is needed for it.
#
# Only the wrapped lomiri-system-settings is replaced; lomiri (the shell), which
# links lomiri-system-settings-unwrapped, is not rebuilt.
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.settingsPanels;

  propertiesFile = "/etc/lomiri-system-settings/properties";

  # Header-only property_get() shim, as <hybris/properties/properties.h>.
  shim = pkgs.runCommand "fp6-android-properties-shim" { } ''
    install -Dm644 ${./android-properties-shim.h} $out/include/hybris/properties/properties.h
  '';

  patchUnwrapped = unwrapped: unwrapped.overrideAttrs (old: {
    pname = "lomiri-system-settings-unwrapped-fp6";
    patches = (old.patches or [ ]) ++ [
      ./lss-build-about-brightness.patch
      ./lss-brightness-define-adapter-slot.patch
    ];
    postPatch = (old.postPatch or "") + ''
      # Battery: visibility is decided by the UPower device count only.
      sed -i '/"visible-if-file-exists".*com\.lomiri\.Repowerd\.conf/d' plugins/battery/battery.settings
      if grep -q visible-if-file-exists plugins/battery/battery.settings; then
        echo "battery.settings still carries visible-if-file-exists" >&2; exit 1
      fi
    '';
    env = (old.env or { }) // {
      NIX_CFLAGS_COMPILE = toString [ (old.env.NIX_CFLAGS_COMPILE or "") "-I${shim}/include" ];
    };
    postInstall = (old.postInstall or "") + ''
      for f in brightness.settings about.settings battery.settings; do
        test -e $out/share/lomiri-system-settings/$f || { echo "missing $f" >&2; exit 1; }
      done
      test -e $out/lib/lomiri-system-settings/libbrightness-plugin.so
      test -e $out/lib/lomiri-system-settings/private/Lomiri/SystemSettings/Brightness/libLomiriBrightnessPanel.so
      test -e $out/lib/lomiri-system-settings/private/Lomiri/SystemSettings/StorageAbout/libLomiriStorageAboutPanel.so
      if grep -q visible-if-file-exists $out/share/lomiri-system-settings/battery.settings; then exit 1; fi
    '';
  });
in {
  options.hardware.fairphone6.settingsPanels = {
    enable = lib.mkEnableOption ''
      the Battery, Brightness and System information panels in
      lomiri-system-settings (built without libhybris)'';
    properties = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = {
        "ro.product.manufacturer" = "Fairphone";
        "ro.product.model" = "FP6";
        "ro.build.display.id" = "NixOS ${config.system.nixos.label}";
        # No Aethercast/Miracast on this port: keeps the page titled
        # "Brightness" and hides the wireless-display rows.
        "ubuntu.widi.supported" = "0";
      };
      description = ''
        Values returned by the property_get() shim (About: vendor/model and
        build id; Brightness: ubuntu.widi.supported). Written to
        ${propertiesFile}.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = lib.mkAfter [ (_final: prev: {
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
        # Only the wrapper is replaced; lomiri itself keeps the stock unwrapped.
        lomiri-system-settings = lprev.lomiri-system-settings.override {
          lomiri-system-settings-unwrapped = patchUnwrapped lprev.lomiri-system-settings-unwrapped;
        };
      });
    }) ];

    environment.etc."lomiri-system-settings/properties".text =
      lib.concatStrings (lib.mapAttrsToList (k: v: "${k}=${v}\n") cfg.properties);

    system.build.fp6-lomiri-system-settings = pkgs.lomiri.lomiri-system-settings;

    assertions = [{
      assertion = config.services.upower.enable;
      message = "The FP6 Battery settings panel needs UPower.";
    }];
  };
}
