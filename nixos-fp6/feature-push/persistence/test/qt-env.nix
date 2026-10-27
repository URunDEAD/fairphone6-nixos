# Laptop (x86) render environment for the Settings > VPN previews:
# qmlscene + Lomiri UI Toolkit + SystemSettings QML + gsettings-qt +
# Lomiri.Connectivity / Lomiri.Settings.Vpn (the stock VPN page's imports)
# and the com.fairphone6.vpn schema (keyfile backend in the preview).
# nix-build test/qt-env.nix -o /tmp/fp6-vpn-qt
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit
              pkgs.lomiri.lomiri-settings-components pkgs.lomiri.gsettings-qt pkgs.lomiri.lomiri-indicator-network
              q.qmenumodel q.qtfeedback ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin ];
  schemas = pkgs.runCommand "fp6-vpn-preview-schemas" { nativeBuildInputs = [ pkgs.glib ]; } ''
    mkdir -p $out
    for p in ${pkgs.lomiri.lomiri-schemas} ${lss}; do
      for d in $p/share/glib-2.0/schemas $p/share/gsettings-schemas/*/glib-2.0/schemas; do
        [ -d "$d" ] && cp -n $d/*.xml $out/ 2>/dev/null || true
      done
    done
    cp ${../com.fairphone6.vpn.gschema.xml} $out/com.fairphone6.vpn.gschema.xml
    glib-compile-schemas --strict $out
  '';
in pkgs.writeShellScriptBin "fp6-qmlscene" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${qmlPath}:${lss}/share/lomiri-system-settings/qml-plugins:${lss}/lib/lomiri-system-settings/private
  export GSETTINGS_SCHEMA_DIR=${schemas}
  export GSETTINGS_BACKEND=keyfile
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${pkgs.makeFontsConf { fontDirectories = [ pkgs.ubuntu-classic or pkgs.dejavu_fonts pkgs.dejavu_fonts ]; }}
  export FP6_LSS=${lss}
  exec ${q.qtdeclarative.dev}/bin/qmlscene "$@"
''
