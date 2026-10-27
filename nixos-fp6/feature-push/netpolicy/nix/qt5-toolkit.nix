{ pkgs }:
# Current nixpkgs defaults Lomiri to Qt 6. The phone and this Rust bridge use Qt 5.
pkgs.lomiri.lomiri-ui-toolkit.override {
  inherit (pkgs.qt5) qmake qtbase qtdeclarative qtgraphicaleffects qtpim
    qtquickcontrols2 qtsvg qtsystems qttools wrapQtAppsHook;
  qtfeedback = pkgs.qt5.callPackage
    (pkgs.path + "/pkgs/development/libraries/qtfeedback") {};
}
