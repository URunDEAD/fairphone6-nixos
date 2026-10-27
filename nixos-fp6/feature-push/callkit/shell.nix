{ pkgs ? import <nixpkgs> {} }:
let
  qt = pkgs.qt5;
  qmlPackages = [ (pkgs.lib.getBin qt.qtdeclarative) qt.qtgraphicaleffects qt.qtquickcontrols pkgs.lomiri.lomiri-ui-toolkit ];
in pkgs.mkShell {
  packages = [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy pkgs.dbus pkgs.python3 ] ++ qmlPackages;
  shellHook = ''
    export QML2_IMPORT_PATH="${pkgs.lib.makeSearchPath qt.qtbase.qtQmlPrefix qmlPackages}"
    export QT_PLUGIN_PATH="${pkgs.lib.makeSearchPath qt.qtbase.qtPluginPrefix [ qt.qtbase (pkgs.lib.getBin qt.qtdeclarative) ]}"
  '';
}
