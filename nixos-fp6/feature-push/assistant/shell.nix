# Development shell for the assistant (cargo test, offscreen QML previews).
{ pkgs ? import (import ../../upstream/mobile-nixos/npins).nixpkgs { } }:
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [ cargo rustc rustfmt clippy pkg-config qt5.qmake dbus ];
  buildInputs = with pkgs; [ sqlite qt5.qtbase qt5.qtdeclarative qt5.qtgraphicaleffects qt5.qtsvg qt5.qtpim
    lomiri.lomiri-ui-toolkit lomiri.lomiri-content-hub ];
  CARGO_HOME = "/tmp/fp6-assistant-cargo";
}
