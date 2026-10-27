# Laptop (x86) preview environment for the FP6 lock screen and wallpapers:
# qmlscene with the real UI toolkit, gsettings-qt (memory backend), the
# com.fairphone6.lockscreen/notifications schemas, the Fp6.Wallpapers plugin
# (built for x86) and the stock LSS QML; Lomiri/LSS C++ parts are QML stubs
# (test/stubs). Used by test/preview.sh.
#   nix-build test -o /tmp/fp6-ls-env && test/preview.sh /tmp/fp6-ls-env OUTDIR
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  schema = pkgs.callPackage ../schema.nix { sensorCenter = 1100.0 / 2484.0; sensorLength = 0.075; };
  plugin = pkgs.callPackage ../wallpapers/package.nix { fp6-lockscreen-schema = schema; };
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              pkgs.lomiri.gsettings-qt pkgs.lomiri.lomiri-settings-components ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin q.qtimageformats ];
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
  schemas = pkgs.runCommand "fp6-ls-test-schemas" { nativeBuildInputs = [ pkgs.glib ]; } ''
    mkdir -p $out
    cp ${schema}/share/gsettings-schemas/*/glib-2.0/schemas/*.xml ${../../notifications/com.fairphone6.notifications.gschema.xml} $out/
    cp ${pkgs.lomiri.lomiri-schemas}/share/gsettings-schemas/*/glib-2.0/schemas/com.lomiri.Shell.gschema.xml $out/ || true
    glib-compile-schemas $out
  '';
in pkgs.writeShellScriptBin "fp6-ls-env" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${./stubs}:${plugin}/lib/lomiri-system-settings/private:${qmlPath}
  export FP6_LSS_QML=${lss}/lib/lomiri-system-settings/private:${lss}/share/lomiri-system-settings/qml-plugins
  export FP6_LSS_BACKGROUND=${lss}/share/lomiri-system-settings/qml-plugins/background
  export FP6_PLUGIN_QML=${plugin}/share/lomiri-system-settings/qml-plugins/background
  export FP6_WALLPAPERS=${pkgs.lomiri.lomiri-wallpapers}/share/wallpapers
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${fonts}
  export GSETTINGS_SCHEMA_DIR=${schemas}
  export GSETTINGS_BACKEND=memory
  export FP6_QMLSCENE=${q.qtdeclarative.dev}/bin/qmlscene
  exec "$@"
''
