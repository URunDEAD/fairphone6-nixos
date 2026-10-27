{ pkgs ? import <nixpkgs> {} }:
let
  qt = pkgs.qt5;
  qmlPackages = [ qt.qtdeclarative qt.qtquickcontrols qt.qtgraphicaleffects
    pkgs.lomiri.lomiri-ui-toolkit pkgs.lomiri.lomiri-content-hub ];
in pkgs.mkShell {
  inputsFrom = [ (pkgs.callPackage ./package.nix {}) ];
  packages = with pkgs; [ cargo rustc rustfmt clippy protobuf cmake pkg-config dbus ];
  shellHook = ''
    export XDG_DATA_DIRS="${pkgs.lomiri.suru-icon-theme}/share:''${XDG_DATA_DIRS:-}"
    export QT_PLUGIN_PATH="${pkgs.lib.makeSearchPath qt.qtbase.qtPluginPrefix [ qt.qtbase qt.qtdeclarative qt.qtwayland qt.qtsvg.bin ]}:''${QT_PLUGIN_PATH:-}"
    export QML2_IMPORT_PATH="${pkgs.lib.makeSearchPath qt.qtbase.qtQmlPrefix qmlPackages}:''${QML2_IMPORT_PATH:-}"
  '';
}
