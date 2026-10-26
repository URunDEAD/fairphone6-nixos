# FP6 ALSA UCM tree: stock alsa-ucm-conf plus the FP6 card files.
{ lib, alsa-ucm-conf, writeText, voiceCallVerb ? true, topMic ? false }:
let
  # Top-level card file: HiFi always, "Voice Call" only when enabled.
  cardConf = writeText "fp6-ucm-card.conf" (''
    Syntax 4

    SectionUseCase."HiFi" {
    	File "/Fairphone/fp6/HiFi.conf"
    	Comment "HiFi quality Music"
    }
  '' + lib.optionalString voiceCallVerb ''

    SectionUseCase."Voice Call" {
    	File "/Fairphone/fp6/VoiceCall.conf"
    	Comment "Phone call (audio via PipeWire)"
    }
  '' + ''

    # pmOS boot state plus explicit music profiles; amps stay <= -2 dB.
    BootSequence [
    	cset "name='Amplifier L Profile Set' music"
    	cset "name='Amplifier R Profile Set' music"
    	cset "name='Amplifier L PCM Playback Volume' 352"
    	cset "name='Amplifier R PCM Playback Volume' 352"
    ]
  '');
in alsa-ucm-conf.overrideAttrs (old: {
  pname = "alsa-ucm-conf-fp6-feature";
  postInstall = (old.postInstall or "") + ''
    for v in HiFi VoiceCall; do
      install -Dm644 ${./ucm/Fairphone/fp6}/$v.conf "$out/share/alsa/ucm2/Fairphone/fp6/$v.conf"
      ${lib.optionalString (!topMic) ''
        # Top mic (AMIC3/ADC2) not wanted; drop it.
        sed -i '/^# BEGIN Mic2/,/^# END Mic2/d' "$out/share/alsa/ucm2/Fairphone/fp6/$v.conf"
        ! grep -q 'Mic2' "$out/share/alsa/ucm2/Fairphone/fp6/$v.conf"
      ''}
    done
    install -Dm644 ${cardConf} "$out/share/alsa/ucm2/conf.d/milos/Fairphone (Gen. 6).conf"
  '';
  passthru = (old.passthru or { }) // { inherit voiceCallVerb topMic; };
})
