{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs {} }:
let
  q = pkgs.libsForQt5;
  panel = import ./panel.nix { inherit pkgs; };
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  dirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit pkgs.lomiri.lomiri-settings-components q.qmenumodel q.qtfeedback ];
  imports = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") dirs;
  plugins = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin ];
in pkgs.writeShellScriptBin "fp6-bt-preview" ''
  export QT_PLUGIN_PATH=${plugins}
  export QML2_IMPORT_PATH=${panel}/lib/lomiri-system-settings/private:${imports}:${lss}/share/lomiri-system-settings/qml-plugins:${lss}/lib/lomiri-system-settings/private
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share
  export FONTCONFIG_FILE=${pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; }}
  export FP6_BT_PANEL=${panel}/share/lomiri-system-settings/qml-plugins/bluetooth
  export QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=10
  unset QT_QUICK_BACKEND
  export QML_DISABLE_DISK_CACHE=1
  export LIBGL_ALWAYS_SOFTWARE=1 QT_STYLE_OVERRIDE=Fusion QT_QPA_PLATFORMTHEME=
  export XDG_CONFIG_HOME=/tmp/fp6-bt-preview-config XDG_CACHE_HOME=/tmp/fp6-bt-preview-cache
  if [ "''${1:-}" = "--test" ]; then
    shift
    exec ${pkgs.xvfb-run}/bin/xvfb-run -a ${q.qtdeclarative.dev}/bin/qmltestrunner "$@"
  fi
  exec ${pkgs.xvfb-run}/bin/xvfb-run -a ${q.qtdeclarative.dev}/bin/qmlscene "$@"
''
