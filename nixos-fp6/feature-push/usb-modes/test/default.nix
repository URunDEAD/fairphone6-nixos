# Laptop (x86) preview environment for System Settings > USB: qmlscene with
# the real Lomiri UI toolkit, lomiri-system-settings' QML, gsettings-qt
# (memory backend) and com.fairphone6.usb-modes.
#   nix-build test -o /tmp/fp6-usbmodes-env && test/preview.sh /tmp/fp6-usbmodes-env OUTDIR
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              pkgs.lomiri.gsettings-qt pkgs.lomiri.lomiri-settings-components ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin ];
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
  schemas = pkgs.runCommand "fp6-usbmodes-test-schemas" { nativeBuildInputs = [ pkgs.glib ]; } ''
    mkdir -p $out
    cp ${../com.fairphone6.usb-modes.gschema.xml} $out/
    glib-compile-schemas --strict $out
  '';
in pkgs.writeShellScriptBin "fp6-usbmodes-env" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${qmlPath}:${lss}/lib/lomiri-system-settings/private:${lss}/share/lomiri-system-settings/qml-plugins
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${fonts}
  export GSETTINGS_SCHEMA_DIR=${schemas}
  export GSETTINGS_BACKEND=memory
  export FP6_QMLSCENE=${q.qtdeclarative.dev}/bin/qmlscene
  exec "$@"
''
