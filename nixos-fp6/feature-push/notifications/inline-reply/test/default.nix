# Laptop (x86) test environment for inline replies:
#  * lomiri-notifications with notifications' app-name patch and this
#    module's patch (its own ctest suite runs in the build, verbose),
#  * fp6-notify-reply (cargo test in the build),
#  * qmlscene / qmltestrunner with the Lomiri UI toolkit and an x86 Lomiri
#    0.5 build (only its Powerd / Utils QML plugins) for the shell QML,
#  * dbus-run-session for the end-to-end D-Bus round (test/e2e.sh).
#   nix-build test -o $SCRATCH/ir-env
{ pkgs ? import (import ../../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  notifications = pkgs.lomiri.lomiri-notifications.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [
      ../../lomiri-notifications-app-name.patch
      ../lomiri-notifications-inline-reply.patch
    ];
    doCheck = true;
    checkPhase = "runHook preCheck; ctest -V; runHook postCheck";
  });
  client = pkgs.callPackage ../package.nix { };
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              pkgs.lomiri.gsettings-qt pkgs.lomiri.lomiri-settings-components notifications
              q.qtmultimedia.bin q.qtmultimedia q.qmenumodel pkgs.lomiri.lomiri ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin ];
  schemas = pkgs.runCommand "fp6-ir-test-schemas" { nativeBuildInputs = [ pkgs.glib ]; } ''
    mkdir -p $out
    cp ${../../com.fairphone6.notifications.gschema.xml} $out/
    # com.lomiri.touch.system etc. (read by the x86 Lomiri plugins)
    cp ${pkgs.lomiri.lomiri-schemas}/share/gsettings-schemas/*/glib-2.0/schemas/*.xml $out/
    glib-compile-schemas --strict $out
  '';
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
in pkgs.writeShellScriptBin "fp6-ir-env" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${qmlPath}
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${fonts}
  export GSETTINGS_SCHEMA_DIR=${schemas}
  export GSETTINGS_BACKEND=memory
  export FP6_QMLSCENE=${q.qtdeclarative.dev}/bin/qmlscene
  export FP6_QMLTESTRUNNER=${q.qtdeclarative.dev}/bin/qmltestrunner
  export FP6_DBUS_RUN_SESSION=${pkgs.dbus}/bin/dbus-run-session
  export FP6_NOTIFY_REPLY=${pkgs.lib.getExe client}
  export FP6_LN=${notifications}
  exec "$@"
''
