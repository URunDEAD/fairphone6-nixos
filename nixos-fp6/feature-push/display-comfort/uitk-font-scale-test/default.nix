# Offscreen check of the UITK font scale (display-comfort fontScaleUitk).
# Runs the stock UITK QML modules under qmlscene (QT_QPA_PLATFORM=offscreen,
# software Qt Quick) with and without the font-scale libLomiriToolkit.so.5
# first on LD_LIBRARY_PATH, exactly as lomiri-session sets it, and compares
# the label pixel sizes and grabs. Also starts a real wrapped UITK app with
# the same environment and checks which libLomiriToolkit it maps.
# Build: system.build.fp6-display-comfort.uitkFontScaleTest.
{ lib, runCommand, makeFontsConf, dejavu_fonts, python3, libsForQt5
, lomiri, uitkFontScaleLib, app ? lomiri.lomiri-calculator-app, appBin ? "lomiri-calculator-app" }:
let
  qt = libsForQt5;
  uitk = lomiri.lomiri-ui-toolkit;
  qml = lib.concatMapStringsSep ":" (p: "${lib.getBin p}/lib/qt-5.15.19/qml") [
    uitk qt.qtdeclarative qt.qtgraphicaleffects qt.qtfeedback qt.qtpim qt.qtsystems qt.qtsvg
  ];
  plugins = lib.concatMapStringsSep ":" (p: "${lib.getBin p}/lib/qt-5.15.19/plugins") [
    qt.qtbase qt.qtdeclarative qt.qtsvg uitk qt.qtpim
  ];
  fonts = makeFontsConf { fontDirectories = [ dejavu_fonts ]; };
in runCommand "fp6-uitk-font-scale-test" { nativeBuildInputs = [ python3 python3.pkgs.pillow ]; } ''
  export HOME=$TMPDIR XDG_RUNTIME_DIR=$TMPDIR/rt
  mkdir -m 700 $XDG_RUNTIME_DIR
  export QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software
  export QML2_IMPORT_PATH=${qml} QT_PLUGIN_PATH=${plugins}
  export FONTCONFIG_FILE=${fonts} GRID_UNIT_PX=8
  mkdir -p $out
  run() {
    name=$1; shift
    mkdir $TMPDIR/$name
    ( cd $TMPDIR/$name && env -u UITK_FONT_SCALE "$@" LD_DEBUG=libs LD_DEBUG_OUTPUT=$TMPDIR/ld-$name \
      timeout 60 ${lib.getDev qt.qtdeclarative}/bin/qmlscene ${./labels.qml} ) \
      > $out/$name.log 2>&1 || { cat $out/$name.log; exit 1; }
    cp $TMPDIR/$name/grab.png $out/$name.png
    grep -h 'calling init: .*libLomiriToolkit' $TMPDIR/ld-$name.* | sed 's/^ *[0-9]*: *//' | sort -u > $out/$name.lib
    grep -o 'UITK-FONT-RESULT .*' $out/$name.log | sed 's/^UITK-FONT-RESULT //' > $out/$name.json
    echo "$name: $(cat $out/$name.json) [$(cat $out/$name.lib)]"
  }
  run stock
  run stock-env13 UITK_FONT_SCALE=1.3
  run shim-unset LD_LIBRARY_PATH=${uitkFontScaleLib}/lib
  run shim-100 LD_LIBRARY_PATH=${uitkFontScaleLib}/lib UITK_FONT_SCALE=1.00
  run shim-130 LD_LIBRARY_PATH=${uitkFontScaleLib}/lib UITK_FONT_SCALE=1.30
  run shim-085 LD_LIBRARY_PATH=${uitkFontScaleLib}/lib UITK_FONT_SCALE=0.85
  run shim-bad LD_LIBRARY_PATH=${uitkFontScaleLib}/lib UITK_FONT_SCALE=7

  # A real wrapped UITK app for 6 s, stock and with the session environment.
  # Both runs must end the same way (timeout's SIGTERM; with no D-Bus and no
  # Mir in the sandbox the app may crash on that teardown, but identically).
  app() {
    name=$1; shift
    rc=0
    env -u UITK_FONT_SCALE "$@" LD_DEBUG=libs LD_DEBUG_OUTPUT=$TMPDIR/ld-$name \
      timeout 6 ${app}/bin/${appBin} > $out/$name.log 2>&1 || rc=$?
    echo $rc > $out/$name.rc
    grep -h 'calling init: .*libLomiriToolkit' $TMPDIR/ld-$name.* | sed 's/^ *[0-9]*: *//' | sort -u > $out/$name.lib
    echo "$name: rc $rc [$(cat $out/$name.lib)]"
  }
  app app-stock
  app app UITK_FONT_SCALE=1.30 LD_LIBRARY_PATH=${uitkFontScaleLib}/lib

  python3 ${./check.py} $out ${uitkFontScaleLib} | tee $out/result.txt
''
