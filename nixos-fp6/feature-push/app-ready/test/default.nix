# Laptop (x86) test environment for Settings > Memory & performance and the
# Lomiri Stage.qml patch (no phone):
#  * fp6-app-ready built for x86 (cargo test runs in the build);
#  * the Settings plugin QML, checked with qmllint;
#  * the target's patched Lomiri Stage.qml (every earlier patch) plus
#    lomiri-memory-predicted.patch, checked with qmllint;
#  * qmlscene with UITK, gsettings-qt (memory backend), the System Settings
#    private QML (SystemSettings, SystemSettings.ListItems).
#   nix-build test -o $SCRATCH/mp-env && test/run-tests.sh $SCRATCH/mp-env OUTDIR
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { }
, psrc ? import ./patched-src.nix { } }:
let
  q = pkgs.libsForQt5;
  lib = pkgs.lib;
  daemon = pkgs.callPackage ../package.nix { inherit (pkgs.lomiri) lomiri-app-launch; };
  # psrc: the kernel6 target's patched Lomiri tree, ours included (patched-src.nix).
  stage = pkgs.runCommand "fp6-memory-stage-qml" { nativeBuildInputs = [ q.qtdeclarative.dev pkgs.patch ]; } ''
    cp -r ${psrc}/qml . && chmod -R u+w qml
    qmllint qml/Stage/Stage.qml
    grep -c 'fp6AppReady.predicted\|fp6AppReady.neverKeep\|model.window.fp6Background)' qml/Stage/Stage.qml
    mkdir -p $out && cp qml/Stage/Stage.qml $out/
  '';
  plugin = pkgs.callPackage ../lomiri-system-settings-app-ready.nix { };
  pluginChecked = pkgs.runCommand "fp6-memory-settings-qmllint" { nativeBuildInputs = [ q.qtdeclarative.dev ]; } ''
    qmllint ${plugin}/share/lomiri-system-settings/qml-plugins/app-launch/*.qml
    touch $out
  '';
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              pkgs.lomiri.gsettings-qt pkgs.lomiri.lomiri-settings-components ];
  qmlPath = lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins")
    [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin ];
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  schemas = pkgs.runCommand "fp6-memory-test-schemas" { nativeBuildInputs = [ pkgs.glib ]; } ''
    mkdir -p $out
    cp ${../com.fairphone6.app-ready.gschema.xml} $out/
    glib-compile-schemas --strict $out
  '';
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
in pkgs.writeShellScriptBin "fp6-mp-env" ''
  unset LD_LIBRARY_PATH
  # ${pluginChecked}
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${qmlPath}
  export FP6_LSS_QML=${lss}/lib/lomiri-system-settings/private:${lss}/share/lomiri-system-settings/qml-plugins
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${fonts}
  export GSETTINGS_SCHEMA_DIR=${schemas}
  export GSETTINGS_BACKEND=memory
  export FP6_QMLSCENE=${q.qtdeclarative.dev}/bin/qmlscene
  export FP6_MP_PLUGIN=${plugin}/share/lomiri-system-settings/qml-plugins/app-launch
  export FP6_MP_STAGE=${stage}/Stage.qml
  export FP6_APP_READY=${daemon}/bin/fp6-app-ready
  exec "$@"
''
