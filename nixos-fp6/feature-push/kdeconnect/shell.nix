{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [ cargo rustc rustfmt clippy pkg-config qt5.qmake dbus xvfb-run ];
  buildInputs = with pkgs; [ openssl qt5.qtbase qt5.qtdeclarative qt5.qtgraphicaleffects qt5.qtsvg lomiri.lomiri-ui-toolkit lomiri.lomiri-content-hub ];
  QT_PLUGIN_PATH = "${pkgs.qt5.qtbase.bin}/${pkgs.qt5.qtbase.qtPluginPrefix}:${pkgs.qt5.qtsvg.bin}/${pkgs.qt5.qtbase.qtPluginPrefix}";
  QML2_IMPORT_PATH = "${pkgs.qt5.qtdeclarative.bin}/${pkgs.qt5.qtbase.qtQmlPrefix}:${pkgs.qt5.qtgraphicaleffects}/${pkgs.qt5.qtbase.qtQmlPrefix}:${pkgs.lomiri.lomiri-ui-toolkit}/${pkgs.qt5.qtbase.qtQmlPrefix}:${pkgs.lomiri.lomiri-content-hub}/${pkgs.qt5.qtbase.qtQmlPrefix}";
  XDG_DATA_DIRS = "${pkgs.lomiri.suru-icon-theme}/share";
  LIBGL_DRIVERS_PATH = "${pkgs.mesa.drivers}/lib/dri";
  CARGO_HOME = "/tmp/lomiri-kdeconnect-cargo";
}
