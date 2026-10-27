# Laptop (x86) builds of the Wi-Fi QR parts and a qmlscene environment for
# the settings-page previews (stock QtMultimedia, no phone camera).
#   nix-build test -A env -o /tmp/fp6-wifiqr-env
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  pkg = pkgs.callPackage ../package.nix { assignedMac = "stable"; };
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback q.qtmultimedia.bin (q.qmenumodel or pkgs.lomiri.qmenumodel) pkgs.lomiri.lomiri-settings-components ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin ];
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
  env = pkgs.writeShellScriptBin "fp6-qmlscene" ''
    export QT_PLUGIN_PATH=${pluginPath}
    export QML2_IMPORT_PATH=${qmlPath}:${lss}/share/lomiri-system-settings/qml-plugins:${lss}/lib/lomiri-system-settings/private:${pkg.lssPlugin}/lib/lomiri-system-settings/private''${FP6_EXTRA_QML:+:$FP6_EXTRA_QML}
    export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
    export FONTCONFIG_FILE=${fonts}
    export FP6_LSS=${lss}
    export FP6_LSS_PLUGIN=${pkg.lssPlugin}
    export FP6_SCANNER=${pkg.scanner}
    export FP6_CORE=${pkg.core}
    exec ${q.qtdeclarative.dev}/bin/qmlscene "$@"
  '';
in { inherit (pkg) core lssPlugin scanner; inherit env; }
