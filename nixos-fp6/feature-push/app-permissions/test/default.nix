# Laptop (x86) preview environment for FP6 app permissions: qmlscene with the
# real UI toolkit, the System Settings QML (SystemSettings, ListItems) and a
# QMenuModel stub (stubs/QMenuModel: a simulated fp6-permission-agent).
#   nix-build test -o /tmp/fp6-perm-env
#   nix-build test/patchcheck.nix -o /tmp/fp6-lomiri-perm
#   test/preview.sh /tmp/fp6-perm-env OUTDIR /tmp/fp6-lomiri-perm [scene ...]
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              pkgs.lomiri.gsettings-qt pkgs.lomiri.lomiri-settings-components ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin q.qtimageformats ];
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
in pkgs.writeShellScriptBin "fp6-perm-env" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${./stubs}:${qmlPath}
  export FP6_LSS_QML=${lss}/lib/lomiri-system-settings/private:${lss}/share/lomiri-system-settings/qml-plugins
  export FP6_LSS_SP=${lss}/share/lomiri-system-settings/qml-plugins/security-privacy
  export FP6_WALLPAPERS=${pkgs.lomiri.lomiri-wallpapers}/share/wallpapers
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${fonts}
  export GSETTINGS_BACKEND=memory
  export FP6_QMLSCENE=${q.qtdeclarative.dev}/bin/qmlscene
  exec "$@"
''
