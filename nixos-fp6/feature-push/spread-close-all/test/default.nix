# Laptop (x86) test environment for the spread close-all: the real Lomiri UI
# toolkit (QtTest, Lomiri.Test), gsettings-qt with the memory backend and the
# app-switcher + app-ready schemas. The Lomiri QML plugins (Lomiri.Gestures,
# Utils) come from an x86 Lomiri build passed to run-tests.sh.
#   nix-build test -o $SCRATCH/sca-env && test/run-tests.sh $SCRATCH/sca-env LOMIRI_X86 PATCHED_TREE OUTDIR
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              pkgs.lomiri.gsettings-qt pkgs.lomiri.lomiri-settings-components ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin ];
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
  schemas = pkgs.runCommand "fp6-sca-test-schemas" { nativeBuildInputs = [ pkgs.glib ]; } ''
    mkdir -p $out
    cp ${../com.fairphone6.app-switcher.gschema.xml} ${../../app-ready/com.fairphone6.app-ready.gschema.xml} \
       ${../../gestures/com.fairphone6.gestures.gschema.xml} $out/
    glib-compile-schemas --strict $out
  '';
in pkgs.writeShellScriptBin "fp6-sca-env" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${qmlPath}
  export FP6_LSS_QML=${lss}/lib/lomiri-system-settings/private:${lss}/share/lomiri-system-settings/qml-plugins
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${fonts}
  export GSETTINGS_SCHEMA_DIR=${schemas}
  export GSETTINGS_BACKEND=memory
  export FP6_QMLSCENE=${q.qtdeclarative.dev}/bin/qmlscene
  export FP6_QMLTESTRUNNER=${q.qtdeclarative.dev}/bin/qmltestrunner
  export FP6_QMLLINT=${q.qtdeclarative.dev}/bin/qmllint
  exec "$@"
''
