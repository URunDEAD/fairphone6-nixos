# Laptop (x86) test environment for the Bluetooth settings: the real FP6
# Bluetooth panel plugin and the patched indicator against python-dbusmock's
# BlueZ template on private D-Bus buses (synthetic devices only).
#   nix-build test -o /tmp/fp6-bt-env && test/preview.py /tmp/fp6-bt-env OUTDIR
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  pkg = pkgs.callPackage ../package.nix { };
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              (q.qmenumodel or pkgs.lomiri.qmenumodel) pkgs.lomiri.lomiri-settings-components ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin ];
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
  python = pkgs.python3.withPackages (ps: [ ps.python-dbusmock ps.dbus-python ps.pygobject3 ]);
in pkgs.writeShellScriptBin "fp6-bt-env" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${pkg.lssBluetooth}/lib/lomiri-system-settings/private:${qmlPath}:${lss}/share/lomiri-system-settings/qml-plugins:${lss}/lib/lomiri-system-settings/private
  export XDG_DATA_DIRS=${pkg.icons}/share:${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share:${pkg.indicator}/share
  export FONTCONFIG_FILE=${fonts}
  export FP6_BT_QML=${pkg.lssBluetooth}/share/lomiri-system-settings/qml-plugins/bluetooth
  export FP6_BT_INDICATOR=${pkg.indicator}/libexec/ayatana-indicator-bluetooth/ayatana-indicator-bluetooth-service
  export FP6_PYTHON=${python}/bin/python3
  export FP6_DBUS_DAEMON=${pkgs.dbus}/bin/dbus-daemon
  export FP6_DBUS_CONF=${pkgs.dbus}/share/dbus-1/session.conf
  export FP6_QMLSCENE=${q.qtdeclarative.dev}/bin/qmlscene
  export GSETTINGS_SCHEMA_DIR=${pkg.indicator}/share/gsettings-schemas/${pkg.indicator.name}/glib-2.0/schemas
  exec "$@"
''
