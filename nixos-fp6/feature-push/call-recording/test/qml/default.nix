# Laptop (x86) preview environment for the call-recording dialer UI:
# qmlscene with the real Lomiri UI Toolkit, gsettings-qt (memory backend,
# com.fairphone6.callrecording compiled), QtContacts (memory manager) and
# Lomiri.Content; QMenuModel, Lomiri.Telephony and Lomiri.Contacts are QML
# stubs with fake data (stubs/).
#   nix-build test/qml -o $TMP/fp6-callrec-qml && test/qml/preview.sh $TMP/fp6-callrec-qml OUTDIR
{ pkgs ? import (import ../../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  qtv = q.qtbase.version;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              pkgs.lomiri.gsettings-qt q.qtpim pkgs.lomiri.lomiri-content-hub ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${qtv}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${qtv}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin q.qtpim ];
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
  schemas = pkgs.runCommand "fp6-callrec-test-schemas" { nativeBuildInputs = [ pkgs.glib ]; } ''
    mkdir -p $out
    cp ${../../com.fairphone6.callrecording.gschema.xml} $out/
    glib-compile-schemas --strict $out
  '';
in pkgs.writeShellScriptBin "fp6-callrec-qml" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${./stubs}:${qmlPath}
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${fonts}
  export GSETTINGS_SCHEMA_DIR=${schemas}
  export GSETTINGS_BACKEND=memory
  export FP6_DIALER_SRC=${(import ../../../../telephony-frontends-packages.nix { inherit pkgs; }).dialer.src}
  export FP6_QMLSCENE=${q.qtdeclarative.dev}/bin/qmlscene
  exec "$@"
''
