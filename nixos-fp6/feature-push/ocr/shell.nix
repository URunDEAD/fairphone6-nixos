{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
  packages = with pkgs; [ cargo rustc rustfmt clippy pkg-config
    (tesseract.override { enableLanguages = [ "eng" "ron" ]; })
    qt5.qtdeclarative qt5.qtgraphicaleffects qt5.qtquickcontrols qt5.qtquickcontrols2 lomiri.lomiri-ui-toolkit
    lomiri.lomiri-content-hub dbus curl time dejavu_fonts
    (python3.withPackages (p: [ p.pillow p.pyqt5 ])) ];
  QT_PLUGIN_PATH = "${pkgs.qt5.qtbase.bin}/lib/qt-${pkgs.qt5.qtbase.version}/plugins";
  QT_QPA_PLATFORM = "offscreen";
  QT_QUICK_BACKEND = "";
  LIBGL_ALWAYS_SOFTWARE = "1";
  QT_STYLE_OVERRIDE = "Fusion";
  QML2_IMPORT_PATH = "${pkgs.qt5.qtgraphicaleffects}/lib/qt-${pkgs.qt5.qtbase.version}/qml:${pkgs.qt5.qtquickcontrols}/lib/qt-${pkgs.qt5.qtbase.version}/qml:${pkgs.lomiri.lomiri-ui-toolkit}/lib/qt-${pkgs.qt5.qtbase.version}/qml:${pkgs.lomiri.lomiri-content-hub}/lib/qt-${pkgs.qt5.qtbase.version}/qml";
}
