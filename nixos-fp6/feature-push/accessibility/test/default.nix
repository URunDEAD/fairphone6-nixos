# Laptop (x86) preview/test environment for the FP6 accessibility module:
# real UITK, gsettings-qt (memory backend), LSS QML, the schemas of this
# module and of display-comfort / quick-settings / notifications, the
# FP6.Accessibility and FP6.DisplayComfort QML plugins, and a GL grabber
# (xvfb + Mesa llvmpipe) for scenes with the colour filter.
#   nix-build test -o $SCRATCH/a11y-env && test/preview.sh $SCRATCH/a11y-env OUTDIR
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { } }:
let
  q = pkgs.libsForQt5;
  a11yQml = pkgs.callPackage ../qml-plugin/package.nix { };
  dcQml = pkgs.callPackage ../../display-comfort/qml-plugin/package.nix { };
  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              pkgs.lomiri.gsettings-qt pkgs.lomiri.lomiri-settings-components a11yQml dcQml ];
  qmlPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = pkgs.lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins") [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin ];
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
  schemas = pkgs.runCommand "fp6-a11y-test-schemas" { nativeBuildInputs = [ pkgs.glib ]; } ''
    mkdir -p $out
    cp ${../com.fairphone6.accessibility.gschema.xml} ${../../display-comfort/com.fairphone6.display-comfort.gschema.xml} \
       ${../../quick-settings/com.fairphone6.quick-settings.gschema.xml} \
       ${../../notifications/com.fairphone6.notifications.gschema.xml} $out/
    glib-compile-schemas --strict $out
  '';
  previewer = pkgs.stdenv.mkDerivation {
    name = "fp6-a11y-previewer";
    dontUnpack = true;
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ q.qtbase q.qtdeclarative ];
    dontWrapQtApps = true;
    buildPhase = ''
      $CXX -std=c++17 -O1 -fPIC -o fp6-a11y-previewer ${./previewer.cpp} \
        $(pkg-config --cflags --libs Qt5Quick Qt5Qml Qt5Gui Qt5Core)
    '';
    installPhase = "install -Dm755 fp6-a11y-previewer $out/bin/fp6-a11y-previewer";
  };
in pkgs.writeShellScriptBin "fp6-a11y-env" ''
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${../../quick-settings/test/stubs}:${qmlPath}
  export FP6_LSS_QML=${lss}/lib/lomiri-system-settings/private:${lss}/share/lomiri-system-settings/qml-plugins
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${fonts}
  export GSETTINGS_SCHEMA_DIR=${schemas}
  export GSETTINGS_BACKEND=memory
  export FP6_QMLSCENE=${q.qtdeclarative.dev}/bin/qmlscene
  export FP6_PREVIEWER=${previewer}/bin/fp6-a11y-previewer
  export FP6_XVFB_RUN=${pkgs.xvfb-run}/bin/xvfb-run
  export LD_LIBRARY_PATH=${pkgs.mesa}/lib __GLX_VENDOR_LIBRARY_NAME=mesa
  export __EGL_VENDOR_LIBRARY_DIRS=${pkgs.mesa}/share/glvnd/egl_vendor.d LIBGL_ALWAYS_SOFTWARE=1
  exec "$@"
''
