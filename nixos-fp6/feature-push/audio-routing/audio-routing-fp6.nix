# FP6 audio output/input switching (Bluetooth A2DP/HFP, USB-C audio, phone
# speaker, earpiece during calls, both microphones).
#
# Root cause of "connected a Bluetooth speaker, audio stays on the phone"
# (results/feature-push-20261024/audio-routing/README.md): the A2DP sink was
# created, but WirePlumber 0.5 keeps a *configured* default sink (+30000
# priority, default-nodes/find-selected-default-node.lua) and the phone's
# configured default is the internal speaker (set by telephony-service, the
# audio tests and pipewire-pulse clients). Nothing ever switches to a new
# device and Lomiri had no UI to choose one.
#
# This module:
#  * ayatana-indicator-sound: "Output" and "Input" sections in the sound
#    menu (one radio item per card port; profile switched when needed);
#  * Settings > Sound: the same lists (actions exported by the indicator);
#  * a manual choice becomes WirePlumber's configured default: it persists,
#    falls back (WirePlumber's default history) while the device is away and
#    returns when it reappears;
#  * "Automatically switch to new devices" (default on): a Bluetooth/USB
#    card whose first node appears becomes the default (done by the
#    indicator, so the toggle in Settings > Sound can turn it off; the
#    internal card's call profile switches never count as new devices);
#  * per-app output/input (Settings > Sound > App audio), kept by the
#    indicator in ~/.config/ayatana-indicator-sound/fp6-audio-routing.ini.
#    WirePlumber's own per-app target memory is off so that "Default" really
#    follows the default device. Call streams stay with telephony-service.
# Volumes stay per device (WirePlumber's route/stream state).
{ config, lib, pkgs, ... }:
let
  cfg = config.hardware.fairphone6.audioRouting;
in {
  options.hardware.fairphone6.audioRouting = {
    enable = lib.mkEnableOption "output/input device switching in the sound indicator and Settings > Sound";
    settingsPage = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Show the Output/Input choice on Settings > Sound.";
    };
  };

  config = lib.mkIf cfg.enable {
    nixpkgs.overlays = [ (final: prev: {
      ayatana-indicator-sound = final.callPackage ./indicator-sound-package.nix {
        ayatana-indicator-sound = prev.ayatana-indicator-sound;
      };
      lomiri = prev.lomiri.overrideScope (_lfinal: lprev: lib.optionalAttrs cfg.settingsPage {
        # The wrapper merges the unwrapped tree with lndir; replace only the
        # Sound page there (nothing is recompiled, other overrides compose).
        lomiri-system-settings = lprev.lomiri-system-settings.overrideAttrs (old: {
          nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ final.buildPackages.patch ];
          postInstall = (old.postInstall or "") + ''
            page=$out/share/lomiri-system-settings/qml-plugins/sound/PageComponent.qml
            cp --remove-destination "$(readlink -f "$page")" "$page"
            chmod u+w "$page"
            patch -p3 "$page" < ${./lss-sound-devices.patch}
            grep -q 'output-devices' "$page"
            install -m644 ${./AppAudioPage.qml} "$(dirname "$page")/AppAudioPage.qml"
          '';
          passthru = (old.passthru or { }) // { fp6AudioDevices = true; };
        });
      });
    }) ];

    # Per-app targets are the indicator's (see above); stream volumes are
    # still restored by WirePlumber.
    services.pipewire.wireplumber.extraConfig."52-fp6-app-routing" = {
      "wireplumber.settings" = { "node.stream.restore-target" = false; };
    };

    environment.systemPackages = [ pkgs.ayatana-indicator-sound ];

    system.build.fp6-audio-routing = {
      indicator = pkgs.ayatana-indicator-sound;
      settings = pkgs.lomiri.lomiri-system-settings;
    };
  };
}
