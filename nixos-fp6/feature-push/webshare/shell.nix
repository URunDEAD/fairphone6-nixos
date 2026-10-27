{ pkgs ? import <nixpkgs> {} }:
let
  qt = pkgs.qt5;
  qml = [ qt.qtdeclarative.bin qt.qtgraphicaleffects qt.qtquickcontrols2.bin pkgs.lomiri.lomiri-ui-toolkit pkgs.lomiri.lomiri-content-hub ];
in pkgs.mkShell {
  packages = [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy pkgs.dbus pkgs.python3 pkgs.xvfb-run qt.qtdeclarative.dev ];
  QML2_IMPORT_PATH = pkgs.lib.makeSearchPath qt.qtbase.qtQmlPrefix qml;
  XDG_DATA_DIRS = "${pkgs.lomiri.suru-icon-theme}/share";
  QT_PLUGIN_PATH = pkgs.lib.makeSearchPath qt.qtbase.qtPluginPrefix [ qt.qtbase.bin qt.qtsvg.bin ];
  shellHook = ''
    export CARGO_HOME="$PWD/.cargo"
  '';
}
