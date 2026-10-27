# FP6 AudioReach topology (Catcrafts audioreach-topology, branch fp6 =
# the pmOS/Nura file the phone runs) + the optional voice-call graph patch.
#   voiceGraph = null : the stock graphs only; must be byte-identical to the
#                       running qcom/milos/Fairphone (Gen. 6)-tplg.bin
#   voiceGraph = path : add the VoiceMMode1 hostless voice graph described by
#                       that file (generated from the stock ACDB)
{ lib, stdenvNoCC, fetchgit, gnum4, alsa-utils
, voiceGraph ? null
, allowExample ? false
}:
stdenvNoCC.mkDerivation {
  pname = "fp6-audioreach-topology" + lib.optionalString (voiceGraph != null) "-voice";
  version = "0-unstable-2026-07-16";
  src = fetchgit {
    url = "https://forgejo.catcrafts.net/Catcrafts/audioreach-topology.git";
    rev = "76e10781fcc16a3fd8dfd0278ad9dc307ed29871";
    hash = "sha256-Z8RI1qMs3kyBZdxTCJ5YLBzJe8+QH56XSplBA0OzKMo=";
  };
  patches = [ ./0001-MILOS-Fairphone-Gen6-optional-hostless-voice-graph.patch ];
  nativeBuildInputs = [ gnum4 alsa-utils ];
  buildPhase = ''
    runHook preBuild
    m4 -I . MILOS-Fairphone-Gen6.m4 > base.conf
    alsatplg -c base.conf -o base-tplg.bin
    ${lib.optionalString (voiceGraph != null) ''
      m4 -I . ${lib.optionalString allowExample "-DFP6_VOICE_ALLOW_EXAMPLE"} \
        -DFP6_VOICE_GRAPH=${voiceGraph} MILOS-Fairphone-Gen6.m4 > voice.conf
      alsatplg -c voice.conf -o voice-tplg.bin
    ''}
    runHook postBuild
  '';
  doCheck = true;
  checkPhase = ''
    # Without a voice graph the patch must not change the phone's topology.
    echo "b799d001cf0da889f92215a6ef5143bda54021f5f76fa3e2e5d3797a169d7aa0  base-tplg.bin" | sha256sum -c -
    ${lib.optionalString (voiceGraph != null) ''
      for s in "VoiceMMode1 Playback" "VoiceMMode1 Capture" "VoiceMMode1 Capture Mixer" "SEN_MI2S_RX Audio Mixer"; do
        grep -aq "$s" voice-tplg.bin
      done
      # VCPM VOICE_CONFIG param id present as a little-endian word
      od -An -tx4 -v voice-tplg.bin | tr -s ' ' '\n' > words.txt
      grep -qx 08001162 words.txt
    ''}
  '';
  installPhase = ''
    d=$out/lib/firmware/qcom/milos
    mkdir -p $d $out/share/fp6-audioreach-topology
    cp base.conf base-tplg.bin $out/share/fp6-audioreach-topology/
    ${if voiceGraph != null then ''
      cp voice.conf voice-tplg.bin $out/share/fp6-audioreach-topology/
      cp voice-tplg.bin "$d/Fairphone (Gen. 6)-tplg.bin"
    '' else ''
      cp base-tplg.bin "$d/Fairphone (Gen. 6)-tplg.bin"
    ''}
  '';
}
