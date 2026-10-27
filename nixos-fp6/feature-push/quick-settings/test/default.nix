# Laptop (x86) preview environment for the FP6 quick settings: qmlscene with
# the real Lomiri UI toolkit, gsettings-qt (memory backend) and the schemas;
# QMenuModel and Lomiri.Indicators are replaced by QML stubs with fake data
# (test/stubs). Used by test/preview.py.
#   nix-build test -o /tmp/fp6-qs-env && test/preview.py /tmp/fp6-qs-env QMLDIR OUTDIR
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              pkgs.lomiri.gsettings-qt pkgs.lomiri.lomiri-settings-components ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin ];
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
  schemas = pkgs.runCommand "fp6-qs-test-schemas" { nativeBuildInputs = [ pkgs.glib ]; } ''
    mkdir -p $out
    cp ${../com.fairphone6.quick-settings.gschema.xml} ${../../display-comfort/com.fairphone6.display-comfort.gschema.xml} \
       ${../../notifications/com.fairphone6.notifications.gschema.xml} $out/
    glib-compile-schemas --strict $out
  '';
in pkgs.writeShellScriptBin "fp6-qs-env" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${./stubs}:${qmlPath}
  export FP6_LSS_QML=${lss}/lib/lomiri-system-settings/private:${lss}/share/lomiri-system-settings/qml-plugins
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${fonts}
  export GSETTINGS_SCHEMA_DIR=${schemas}
  export GSETTINGS_BACKEND=memory
  export FP6_QMLSCENE=${q.qtdeclarative.dev}/bin/qmlscene
  exec "$@"
''
