{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
  packages = with pkgs; [ cargo rustc rustfmt clippy cmake pkg-config clang llvmPackages.libclang
    espeak-ng ffmpeg time curl python3 dbus qt5.qtdeclarative qt5.qtquickcontrols2 qt5.qtgraphicaleffects
    lomiri.lomiri-ui-toolkit xvfb-run mesa ];
  LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
  CARGO_HOME = toString ./.cargo-home;
  QT_PLUGIN_PATH = "${pkgs.qt5.qtbase.bin}/${pkgs.qt5.qtbase.qtPluginPrefix}";
  QML2_IMPORT_PATH = pkgs.lib.makeSearchPath pkgs.qt5.qtbase.qtQmlPrefix [ pkgs.qt5.qtdeclarative pkgs.qt5.qtquickcontrols2 pkgs.qt5.qtgraphicaleffects pkgs.lomiri.lomiri-ui-toolkit ];
  LIBGL_DRIVERS_PATH = "${pkgs.mesa}/lib/dri";
}
