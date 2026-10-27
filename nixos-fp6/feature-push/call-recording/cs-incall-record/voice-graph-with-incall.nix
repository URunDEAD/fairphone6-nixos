# Off-by-default composition: cs-call-audio's voice graph description with
# the in-call record (and optionally in-call music) front ends appended.
# Nothing imports this file. It does not change cs-call-audio's files: the
# result is just another value for their
# hardware.fairphone6.csCallAudio.voiceGraph option (a path to an m4
# description that their topology/package.nix passes as -DFP6_VOICE_GRAPH).
#
#   nix-build nixos-fp6/feature-push/call-recording/cs-incall-record/voice-graph-with-incall.nix -A graph
#   nix-build nixos-fp6/feature-push/call-recording/cs-incall-record/voice-graph-with-incall.nix -A topology
#
# recordDir: 1 uplink only, 2 downlink only, 3 UL+DL mono, 4 UL+DL stereo
#            (L = uplink, R = downlink). Opening the PCM with 1 channel on
#            the stereo build gives an UL+DL mix (the stream MFC downmixes).
# music:     also add the in-call music FE (MultiMedia4 Playback) that is
#            mixed into the uplink (announcement tone). Experimental.
{ nixpkgs ? (import ../../../upstream/mobile-nixos/npins).nixpkgs
, pkgs ? import nixpkgs { }
, voiceGraph ? ../../cs-call-audio/topology/milos-voice-graph.m4
, recordDir ? 4
, music ? false
}:
assert builtins.elem recordDir [ 1 2 3 4 ];
let
  graph = pkgs.runCommand "fp6-voice-graph-incall.m4" { } ''
    {
      echo "define(\`FP6_INCALL_RECORD_DIR', \`${toString recordDir}')dnl"
      ${pkgs.lib.optionalString music ''echo "define(\`FP6_INCALL_MUSIC', \`1')dnl"''}
      cat ${voiceGraph}
      cat ${./cs-incall-record.m4}
    } > $out
  '';
in
{
  inherit graph;
  # The FP6 topology with voice + in-call graphs (their package, read-only use).
  topology = pkgs.callPackage ../../cs-call-audio/topology/package.nix {
    voiceGraph = graph;
  };
}
