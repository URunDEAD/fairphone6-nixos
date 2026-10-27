# Laptop (x86) render environment for the audio routing previews:
# qmlscene + Lomiri UI Toolkit + lomiri-settings-components (Menus) +
# QMenuModel + the stock SystemSettings QML module + Lomiri's indicator QML,
# and a Python with PyGObject for the mock indicator service.
# nix-build test/qt-env.nix -o /tmp/fp6-audio-qt
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit
              pkgs.lomiri.lomiri-settings-components q.qmenumodel pkgs.lomiri.gsettings-qt pkgs.lomiri.lomiri-indicator-network pkgs.lomiri.lomiri-thumbnailer q.qtfeedback or null q.qtmultimedia.bin or null ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") (pkgs.lib.filter (p: p != null) qmlDirs);
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin ];
  schemas = pkgs.runCommand "fp6-audio-preview-schemas" { nativeBuildInputs = [ pkgs.glib ]; } ''
    mkdir -p $out
    for p in ${pkgs.lomiri.lomiri-schemas} ${pkgs.lomiri.lomiri-system-settings-unwrapped} ${pkgs.maliit-keyboard} ${pkgs.lomiri.lomiri}; do
      for d in $p/share/glib-2.0/schemas $p/share/gsettings-schemas/*/glib-2.0/schemas; do
        [ -d "$d" ] && cp -n $d/*.xml $out/ 2>/dev/null || true
      done
    done
    glib-compile-schemas $out
  '';
  python = pkgs.python3.withPackages (ps: [ ps.pygobject3 ]);
in pkgs.writeShellScriptBin "fp6-qmlscene" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${qmlPath}:${lss}/share/lomiri-system-settings/qml-plugins:${lss}/lib/lomiri-system-settings/private:${pkgs.lomiri.lomiri}/lib/qt-${q.qtbase.version}/qml
  export GSETTINGS_SCHEMA_DIR=${schemas}
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${pkgs.makeFontsConf { fontDirectories = [ pkgs.ubuntu-classic or pkgs.dejavu_fonts pkgs.dejavu_fonts ]; }}
  export FP6_PYTHON=${python}/bin/python3
  export FP6_GI_TYPELIB_PATH=${pkgs.glib.out}/lib/girepository-1.0:${pkgs.gobject-introspection}/lib/girepository-1.0
  export FP6_LSS=${lss}
  export FP6_LOMIRI=${pkgs.lomiri.lomiri}
  exec ${q.qtdeclarative.dev}/bin/qmlscene "$@"
''
