{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
  packages = with pkgs; [ cargo rustc rustfmt clippy pkg-config xvfb-run qt5.qtbase qt5.qtdeclarative qt5.qtgraphicaleffects ];
  buildInputs = with pkgs; [ qt5.qtbase qt5.qtdeclarative qt5.qtgraphicaleffects ];
  nativeBuildInputs = [ pkgs.qt5.wrapQtAppsHook ];
  QT_PLUGIN_PATH = "${pkgs.qt5.qtbase.bin}/${pkgs.qt5.qtbase.qtPluginPrefix}";
  QML2_IMPORT_PATH = "${pkgs.qt5.qtdeclarative.bin}/${pkgs.qt5.qtbase.qtQmlPrefix}:${pkgs.qt5.qtgraphicaleffects}/${pkgs.qt5.qtbase.qtQmlPrefix}:${pkgs.lomiri.lomiri-ui-toolkit}/${pkgs.qt5.qtbase.qtQmlPrefix}";
  shellHook = ''
    export XDG_DATA_DIRS="${pkgs.lomiri.suru-icon-theme}/share:''${XDG_DATA_DIRS:-}"
  '';
}
