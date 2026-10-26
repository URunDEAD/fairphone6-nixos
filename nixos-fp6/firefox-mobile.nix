{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.mobileFirefox;
  mobileConfig = pkgs.stdenvNoCC.mkDerivation {
    pname = "mobile-config-firefox-fp6";
    version = "5.5.0-git-84913a3-fp6.2";
    src = ./firefox-mobile-config;
    dontBuild = true;
    installPhase = ''
      mkdir -p "$out/share/mobile-config-firefox" "$out/share/doc/mobile-config-firefox"
      cp -r src/modules/. "$out/share/mobile-config-firefox/"
      cp -r src/themes "$out/share/mobile-config-firefox/"
      cp LICENSE UPSTREAM.json "$out/share/doc/mobile-config-firefox/"
    '';
    meta.license = lib.licenses.mpl20;
  };
  autoConfig = pkgs.writeText "fp6-mobile-config-autoconfig.js"
    (builtins.replaceStrings [ "/usr/lib/mobile-config-firefox/chrome.manifest" ]
      [ "${mobileConfig}/share/mobile-config-firefox/chrome.manifest" ]
      (builtins.readFile ./firefox-mobile-config/src/mobile-config-autoconfig.js));
  firefox = pkgs.firefox.override {
    extraAutoConfig = ''pref("general.config.sandbox_enabled", false);'';
    extraPrefsFiles = [ autoConfig ];
    extraPrefs = ''
      defaultPref("browser.uidensity", 2);
      defaultPref("media.webrtc.camera.allow-pipewire", true);
      defaultPref("widget.wayland.text-input-v3.enabled", true);
      // Honor device-width metadata and scale legacy pages as a mobile browser.
      // This does not hide horizontal overflow in a website's own layout.
      defaultPref("dom.meta-viewport.enabled", true);
      // The phone's primary input is its touchscreen. GTK's desktop pointer
      // classification otherwise makes touch-adaptive sites choose wide pages.
      // Leave allPointerCapabilities native for optional external input devices.
      defaultPref("ui.primaryPointerCapabilities", 1);
    '';
  };
in {
  options.hardware.fairphone6.mobileFirefox.enable = lib.mkEnableOption
    "release Firefox with the upstream mobile interface and touch configuration";
  config = lib.mkIf cfg.enable {
    hardware.fairphone6.postmarketosApps.browserPackage = firefox;
    environment.systemPackages = [ mobileConfig ];
  };
}
