# Laptop (x86) render environment for the settings-page previews:
# qmlscene + Lomiri UI Toolkit + the stock SystemSettings QML module +
# Suru icons. nix-build test/qt-env.nix -> result/bin/fp6-qmlscene
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback or null q.qtmultimedia.bin or null ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") (pkgs.lib.filter (p: p != null) qmlDirs);
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin ];
  plugin = pkgs.callPackage ../package.nix { };
in pkgs.writeShellScriptBin "fp6-qmlscene" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${qmlPath}:${lss}/share/lomiri-system-settings/qml-plugins:${plugin.lssPlugin}/lib/lomiri-system-settings/private
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${pkgs.makeFontsConf { fontDirectories = [ pkgs.ubuntu-classic or pkgs.dejavu_fonts pkgs.dejavu_fonts ]; }}
  export FP6_LSS_PLUGIN=${plugin.lssPlugin}
  export FP6_AGENT=${plugin.core}
  exec ${q.qtdeclarative.dev}/bin/qmlscene "$@"
''
