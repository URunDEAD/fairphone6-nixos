# Userspace parts of the FP6 sound feature, for the ARM builder:
#   .macmini-builder/nix-remote build --eval-store daemon \
#     --file nixos-fp6/feature-push/audio/build.nix --no-link --print-out-paths
# ucm:       the UCM tree the module points PipeWire at
# ucmCheck:  alsa-lib parse of every FP6 UCM file + amplifier volume cap check
# telephony: lomiri-telephony-service with the UCM port-name patch
# imsd:      imsd with media.role=Phone call streams
{ nixpkgs ? (import ../../upstream/mobile-nixos/npins).nixpkgs }:
let
  pkgs = import nixpkgs { system = "aarch64-linux"; config.allowUnfree = true; };
  ucm = pkgs.callPackage ./ucm-package.nix { };
  ucmTopMic = pkgs.callPackage ./ucm-package.nix { topMic = true; };
  ucmCheck = pkgs.runCommandCC "fp6-ucm-syntax-check" { buildInputs = [ pkgs.alsa-lib ]; } ''
    cat > check.c <<'C'
    #include <alsa/asoundlib.h>
    int main(int argc, char **argv) {
      for (int i = 1; i < argc; i++) {
        snd_config_t *top; snd_input_t *in; int err;
        if (snd_config_top(&top) < 0) return 2;
        if ((err = snd_input_stdio_open(&in, argv[i], "r")) < 0) { fprintf(stderr, "%s: %s\n", argv[i], snd_strerror(err)); return 1; }
        err = snd_config_load(top, in); snd_input_close(in);
        if (err < 0) { fprintf(stderr, "%s: parse error %s\n", argv[i], snd_strerror(err)); return 1; }
        printf("parsed %s\n", argv[i]); snd_config_delete(top);
      }
      return 0;
    }
    C
    $CC check.c -o check -lasound
    d=${ucm}/share/alsa/ucm2
    ./check "$d/Fairphone/fp6/HiFi.conf" "$d/Fairphone/fp6/VoiceCall.conf" "$d/conf.d/milos/Fairphone (Gen. 6).conf" > $out
    t=${ucmTopMic}/share/alsa/ucm2
    ./check "$t/Fairphone/fp6/HiFi.conf" "$t/Fairphone/fp6/VoiceCall.conf" >> $out
    grep -q 'SectionDevice."Mic2"' "$t/Fairphone/fp6/HiFi.conf"
    ! grep -q ConflictingDevice "$d/Fairphone/fp6/HiFi.conf" "$d/Fairphone/fp6/VoiceCall.conf"
    grep -q 'SectionUseCase."Voice Call"' "$d/conf.d/milos/Fairphone (Gen. 6).conf"
    # Safety: no amplifier volume above the pmOS-tested 352 (-2 dB) anywhere.
    if grep -rhoE "Amplifier [LR] PCM Playback Volume' [0-9]+" "$d/Fairphone/fp6" "$d/conf.d/milos" \
        | awk '{ if ($NF > 352) bad = 1 } END { exit !bad }'; then
      echo "amplifier volume above 352" >&2; exit 1
    fi
    # Every receiver-profile switch happens with the amp at 0 first.
    grep -B1 "Profile Set' receiver" "$d/Fairphone/fp6/HiFi.conf" | grep -q "Volume' 0"
    echo "amp volume cap ok" >> $out
  '';
  telephony = pkgs.lomiri.lomiri-telephony-service.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./telephony-service-ucm-port-names.patch ];
  });
  imsd = (pkgs.callPackage ../../imsd-package.nix { }).overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./imsd-media-role-phone.patch ];
  });
in { inherit ucm ucmTopMic ucmCheck telephony imsd; }
