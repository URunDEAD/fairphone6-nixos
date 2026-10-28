# Laptop (x86) test environment for the FP6 lock sounds:
#  * the module's x86 packages (fp6-lock-sounds with cargo test in the build,
#    the converted bundle, the shared QML, the Settings files);
#  * trees: the target's patched Lomiri qml/ (test/patched-src.nix, this
#    module off) with this module's postPatch applied, and the Sound page as
#    the target builds it (stock + audio-routing's patch + this module's
#    postInstall); both are checked with qmllint while building;
#  * qmlscene / qmltestrunner with UITK, gsettings-qt (memory backend),
#    QMenuModel, QtMultimedia and the x86 Lomiri Powerd plugin;
#  * dbus-run-session + python3-gobject for test/mock-bus.py, and PipeWire,
#    pipewire-pulse and WirePlumber for the audio round (test/audio-e2e.sh).
#   nix-build test -o $SCRATCH/ls-env
{ pkgs ? import (import ../../../upstream/mobile-nixos/npins).nixpkgs { }
, psrc ? import ./patched-src.nix { } }:
let
  q = pkgs.libsForQt5;
  lib = pkgs.lib;
  pkg = pkgs.callPackage ../package.nix { home = "/home/user"; };

  lomiriTree = pkgs.runCommand "fp6-lock-sounds-lomiri-qml" { nativeBuildInputs = [ q.qtdeclarative.dev ]; } ''
    cp -r ${psrc}/qml . && chmod -R u+w qml
    ${pkg.lomiriPostPatch { withDnd = true; }}
    qmllint qml/Shell.qml qml/Components/Fp6LockSounds.qml qml/Components/Fp6LockSoundPlayer.qml
    grep -A6 'FP6 lock sounds' qml/Shell.qml
    cp -r qml $out
  '';
  lssSoundPage = pkgs.runCommand "fp6-lock-sounds-lss-sound-page" { nativeBuildInputs = [ q.qtdeclarative.dev pkgs.patch ]; } ''
    mkdir -p $out/share/lomiri-system-settings/qml-plugins
    cp -r ${pkgs.lomiri.lomiri-system-settings-unwrapped}/share/lomiri-system-settings/qml-plugins/sound \
      $out/share/lomiri-system-settings/qml-plugins/
    chmod -R u+w $out
    page=$out/share/lomiri-system-settings/qml-plugins/sound/PageComponent.qml
    # as audio-routing-fp6.nix does, before this module
    patch -p3 "$page" < ${../../audio-routing/lss-sound-devices.patch}
    ${pkg.lssPostInstall}
    qmllint $out/share/lomiri-system-settings/qml-plugins/sound/*.qml
  '';

  qmlDirs = [ q.qtdeclarative.bin q.qtgraphicaleffects q.qtsvg.bin pkgs.lomiri.lomiri-ui-toolkit q.qtfeedback
              pkgs.lomiri.gsettings-qt pkgs.lomiri.lomiri-settings-components q.qtmultimedia.bin q.qtmultimedia
              q.qmenumodel pkgs.lomiri.lomiri ];
  qmlPath = lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/qml") qmlDirs;
  pluginPath = lib.concatMapStringsSep ":" (p: "${p}/lib/qt-${q.qtbase.version}/plugins")
    [ q.qtbase.bin q.qtsvg.bin q.qtdeclarative.bin q.qtmultimedia.bin q.qtmultimedia ];
  lss = pkgs.lomiri.lomiri-system-settings-unwrapped;
  schemas = pkgs.runCommand "fp6-ls-test-schemas" { nativeBuildInputs = [ pkgs.glib ]; } ''
    mkdir -p $out
    cp ${../com.fairphone6.lock-sounds.gschema.xml} ${../../notifications/com.fairphone6.notifications.gschema.xml} $out/
    cp ${pkgs.lomiri.lomiri-schemas}/share/gsettings-schemas/*/glib-2.0/schemas/*.xml $out/
    glib-compile-schemas --strict $out
  '';
  fonts = pkgs.makeFontsConf { fontDirectories = [ (pkgs.ubuntu-classic or pkgs.dejavu_fonts) pkgs.dejavu_fonts ]; };
  python = pkgs.python3.withPackages (p: [ p.pygobject3 ]);
  gst = with pkgs.gst_all_1; [ gstreamer gst-plugins-base gst-plugins-good ]; # Audio fallback: decodebin, vorbis, pulsesink
in pkgs.writeShellScriptBin "fp6-ls-env" ''
  unset LD_LIBRARY_PATH
  export QT_PLUGIN_PATH=${pluginPath}
  export QML2_IMPORT_PATH=${qmlPath}
  export FP6_LSS_QML=${lss}/lib/lomiri-system-settings/private:${lss}/share/lomiri-system-settings/qml-plugins
  export XDG_DATA_DIRS=${pkgs.lomiri.suru-icon-theme}/share:${pkgs.hicolor-icon-theme}/share
  export FONTCONFIG_FILE=${fonts}
  export GSETTINGS_SCHEMA_DIR=${schemas}
  export GSETTINGS_BACKEND=memory
  export GST_PLUGIN_SYSTEM_PATH_1_0=${lib.concatMapStringsSep ":" (p: "${lib.getLib p}/lib/gstreamer-1.0") gst}
  export GI_TYPELIB_PATH=${pkgs.glib.out}/lib/girepository-1.0:${pkgs.gobject-introspection}/lib/girepository-1.0
  export FP6_QMLSCENE=${q.qtdeclarative.dev}/bin/qmlscene
  export FP6_QMLTESTRUNNER=${q.qtdeclarative.dev}/bin/qmltestrunner
  export FP6_DBUS_RUN_SESSION=${pkgs.dbus}/bin/dbus-run-session
  export FP6_PYTHON=${python}/bin/python3
  export FP6_LOCK_SOUNDS=${lib.getExe pkg.tool}
  export FP6_GSETTINGS_BIN=${lib.getBin pkgs.glib}/bin/gsettings
  export FP6_LS_RINGTONE=${pkgs.lomiri.lomiri-sounds}/share/sounds/lomiri/ringtones/Ubuntu.ogg
  export FP6_LS_BUNDLE=${pkg.sounds}/share/sounds/fp6-lock
  export FP6_LS_LOMIRI_QML=${lomiriTree}
  export FP6_LS_LSS=${lssSoundPage}/share/lomiri-system-settings/qml-plugins/sound
  export FP6_PIPEWIRE=${pkgs.pipewire}
  export FP6_WIREPLUMBER=${pkgs.wireplumber}
  export FP6_PULSE=${pkgs.pulseaudio}
  export FP6_MAGICK=${pkgs.imagemagick}/bin/magick
  exec "$@"
''
