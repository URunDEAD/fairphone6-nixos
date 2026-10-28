# Laptop preview environment for the Assistant UI: the real UI toolkit,
# Content Hub QML, QtOrganizer (qtpim), fonts and icons.
#   nix-build test -o /tmp/fp6-assistant-env
#   test/preview.sh /tmp/fp6-assistant-env rust/target/debug/fp6-assistant OUTDIR [scene ...]
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin q.qtpim pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              pkgs.lomiri.lomiri-content-hub ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin q.qtimageformats q.qtpim ];
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
in pkgs.writeShellScriptBin "fp6-assistant-env" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${qmlPath}
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${fonts}
  export FP6_MAGICK=${pkgs.imagemagick}/bin/magick
  exec "$@"
''
