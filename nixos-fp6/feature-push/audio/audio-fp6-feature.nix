# FP6 sound feature module (speaker, earpiece, both microphones, call routing).
# Add to a target with extendModules (see trial-lomiri-audio-fp6.nix). It
# builds on ../../audio-fp6.nix (pmOS UCM, PipeWire, WirePlumber buffering),
# imports it so it also works on targets that do not, and replaces only the
# UCM tree it points PipeWire at. Activation runs no mixer commands; UCM
# applies its sequences when WirePlumber opens the card/profile.
{ config, lib, pkgs, options, ... }:
let
  cfg = config.hardware.fairphone6.audioFeature;

  ucmPackage = pkgs.callPackage ./ucm-package.nix { inherit (cfg) voiceCallVerb topMic; };
  ucmPath = "${ucmPackage}/share/alsa/ucm2";

  audioServices = lib.genAttrs [ "pipewire" "pipewire-pulse" "wireplumber" ] (_: {
    environment.ALSA_CONFIG_UCM2 = lib.mkForce ucmPath;
  });

  telephonyOverlay = _final: prev: {
    lomiri = prev.lomiri.overrideScope (_lfinal: lprev: {
      lomiri-telephony-service = lprev.lomiri-telephony-service.overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [ ./telephony-service-ucm-port-names.patch ];
      });
    });
  };

  hasImsd = options ? services && options.services ? fp6Imsd;
in
{
  imports = [ ../../audio-fp6.nix ];

  options.hardware.fairphone6.audioFeature = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "FP6 speaker/earpiece/dual-mic UCM and call routing.";
    };
    voiceCallVerb = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Add the routing-only UCM "Voice Call" verb (amp call profiles,
        earpiece default). Telephony-service switches to it during calls.
      '';
    };
    topMic = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Expose the top microphone (AMIC3 via WCD9378 ADC2) as UCM device
        Mic2. Its UCM reads ADC2 from SWR_MIC0, because the qcom SoundWire
        master ignores the codec's master channel map (see
        results/feature-push-20261024/audio/mic2-README.md).
      '';
    };
    telephonyPortNames = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Patch lomiri-telephony-service to recognise "[Out] Earpiece",
        "[Out] Speaker", "[In] Mic1" (PipeWire/PulseAudio UCM port names).
      '';
    };
    imsdMediaRole = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        If the imsd module is present, default its package to one whose
        pw-play/pw-record streams carry media.role=Phone.
      '';
    };
  };

  config = lib.mkIf cfg.enable (lib.mkMerge [
    {
      environment.variables.ALSA_CONFIG_UCM2 = lib.mkForce ucmPath;
      systemd.user.services = lib.mkIf (!config.services.pipewire.systemWide) audioServices;
      systemd.services = lib.mkIf config.services.pipewire.systemWide audioServices;
      system.build.fp6AudioUcm = ucmPackage;

      nixpkgs.overlays = lib.mkIf cfg.telephonyPortNames (lib.mkAfter [ telephonyOverlay ]);
    }
    (lib.optionalAttrs hasImsd {
      services.fp6Imsd.package = lib.mkIf cfg.imsdMediaRole (lib.mkDefault
        ((pkgs.callPackage ../../imsd-package.nix { }).overrideAttrs (old: {
          patches = (old.patches or [ ]) ++ [ ./imsd-media-role-phone.patch ];
        })));
    })
  ]);
}
