# Laptop dev/test shell for fp6-callrec (pinned nixpkgs, same as the phone
# build): cargo, the encoders, PipeWire tools, python-dbusmock.
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [ cargo rustc clippy pkg-config ];
  buildInputs = with pkgs; [ glib sqlite alsa-lib ];
  packages = with pkgs; [
    flac opus-tools pipewire wireplumber sox dbus glib
    (python3.withPackages (p: [ p.python-dbusmock p.dbus-python p.pygobject3 ]))
  ];
}
