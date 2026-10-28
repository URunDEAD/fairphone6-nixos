# Laptop (x86) preview environment for FP6 charging feedback: qmlscene with
# the real UI toolkit, gsettings-qt (memory backend) and the schemas; Powerd,
# QMenuModel (fp6-charging-feedback + sound indicator) and NotificationAudio
# are QML stubs (test/stubs, test/NotificationAudio.qml).
#   nix-build test -o /tmp/fp6-cf-env
#   nix-build test/patchcheck.nix -o /tmp/fp6-lomiri-cf
#   test/preview.sh /tmp/fp6-cf-env OUTDIR /tmp/fp6-lomiri-cf [scene ...]
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              pkgs.lomiri.gsettings-qt pkgs.lomiri.lomiri-settings-components ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin q.qtimageformats ];
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
  schemas = pkgs.runCommand "fp6-cf-test-schemas" { nativeBuildInputs = [ pkgs.glib ]; } ''
    mkdir -p $out
    cp ${../com.fairphone6.charging-feedback.gschema.xml} ${../../notifications/com.fairphone6.notifications.gschema.xml} $out/
    glib-compile-schemas --strict $out
  '';
in pkgs.writeShellScriptBin "fp6-cf-env" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${./stubs}:${qmlPath}
  export FP6_LSS_QML=${lss}/lib/lomiri-system-settings/private:${lss}/share/lomiri-system-settings/qml-plugins
  export FP6_LSS_BATTERY=${lss}/share/lomiri-system-settings/qml-plugins/battery
  export FP6_WALLPAPERS=${pkgs.lomiri.lomiri-wallpapers}/share/wallpapers
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${fonts}
  export GSETTINGS_SCHEMA_DIR=${schemas}
  export GSETTINGS_BACKEND=memory
  export FP6_QMLSCENE=${q.qtdeclarative.dev}/bin/qmlscene
  export FP6_MAGICK=${pkgs.imagemagick}/bin/magick
  exec "$@"
''
