{ pkgs ? import <nixpkgs> {}, withPiper ? false }:
pkgs.mkShell {
  packages = with pkgs; [ python3 curl time dbus espeak-ng ffmpeg sherpa-onnx ] ++ pkgs.lib.optional withPiper pkgs.piper-tts;
  # The evaluation-only Vosk wheel's libvosk uses the normal C++ runtime.
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [ pkgs.stdenv.cc.cc.lib ];
}
