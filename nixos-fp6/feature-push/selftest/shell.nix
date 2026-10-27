{ pkgs ? import <nixpkgs> {} }:
let toolkit = import ./nix/qt5-toolkit.nix { inherit pkgs; };
in pkgs.mkShell {
  packages = with pkgs; [ cargo rustc rustfmt clippy pkg-config qt5.qtbase qt5.qtdeclarative qt5.qtquickcontrols2 qt5.qtgraphicaleffects qt5.qtsvg toolkit dbus glib python3 xvfb-run mesa ];
  QT_QPA_PLATFORM = "offscreen";
  UITK_ICON_THEME = "suru";
  XDG_DATA_DIRS = "${pkgs.lomiri.suru-icon-theme}/share";
  QT_PLUGIN_PATH = pkgs.lib.makeSearchPathOutput "bin" pkgs.qt5.qtbase.qtPluginPrefix [ pkgs.qt5.qtbase pkgs.qt5.qtsvg ];
  QML2_IMPORT_PATH = pkgs.lib.makeSearchPathOutput "bin" pkgs.qt5.qtbase.qtQmlPrefix [ toolkit pkgs.qt5.qtdeclarative pkgs.qt5.qtgraphicaleffects pkgs.qt5.qtquickcontrols2 ];
  shellHook = ''
    export CARGO_HOME=/tmp/fp6-selftest-cargo
    export CARGO_TARGET_DIR=/tmp/fp6-selftest-target
  '';
}
