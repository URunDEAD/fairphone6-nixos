# Offscreen check of the UITK font scale (display-comfort fontScaleUitk).
# Runs the stock UITK QML modules under qmlscene (QT_QPA_PLATFORM=offscreen,
# software Qt Quick) with and without the font-scale libLomiriToolkit.so.5
# first on LD_LIBRARY_PATH, exactly as lomiri-session sets it, and compares
# the label pixel sizes and grabs. Live runs change the font-scale file while
# the scene runs (the size follows without a restart). Also starts a real wrapped UITK app with
# the same environment and checks which libLomiriToolkit it maps.
# Build: system.build.fp6-display-comfort.uitkFontScaleTest.
{ lib, runCommand, makeFontsConf, dejavu_fonts, ubuntu-classic, python3, libsForQt5
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
  fonts = makeFontsConf { fontDirectories = [ ubuntu-classic dejavu_fonts ]; };
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
    ( cd $TMPDIR/$name && env -u UITK_FONT_SCALE -u UITK_FONT_BOLD "$@" LD_DEBUG=libs LD_DEBUG_OUTPUT=$TMPDIR/ld-$name \
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
  # Bold text (UITK_FONT_BOLD=1): default weight Light -> Normal.
  run stock-bold UITK_FONT_BOLD=1
  run shim-bold LD_LIBRARY_PATH=${uitkFontScaleLib}/lib UITK_FONT_BOLD=1
  run shim-bold0 LD_LIBRARY_PATH=${uitkFontScaleLib}/lib UITK_FONT_BOLD=0

  # Live changes: the font-scale file is written (or removed) while the
  # scene runs, after it has printed its first sizes.
  # live NAME START CHANGE [ENV...]: START = file content at start (- none),
  # CHANGE = - (none) / rm / inplace:VALUE / VALUE (atomic rename, like the daemon).
  live() {
    name=$1 start=$2 change=$3; shift 3
    d=$TMPDIR/$name; cfg=$d/cfg/lomiri-ui-toolkit
    mkdir -p $cfg
    [ "$start" = - ] || echo "$start" > $cfg/font-scale
    ( cd $d && env -u UITK_FONT_SCALE XDG_CONFIG_HOME=$d/cfg "$@" \
      timeout 60 ${lib.getDev qt.qtdeclarative}/bin/qmlscene ${./live.qml} ) > $out/$name.log 2>&1 &
    pid=$!
    for i in $(seq 300); do grep -q UITK-LIVE-READY $out/$name.log && break; sleep 0.1; done
    case $change in
      -) ;;
      rm) rm -f $cfg/font-scale ;;
      inplace:*) echo "''${change#inplace:}" > $cfg/font-scale ;;
      *) echo "$change" > $cfg/font-scale.fp6-tmp && mv $cfg/font-scale.fp6-tmp $cfg/font-scale ;;
    esac
    wait $pid || { cat $out/$name.log; exit 1; }
    cp $d/before.png $out/$name-before.png; cp $d/after.png $out/$name-after.png
    grep -o 'UITK-LIVE-BEFORE .*' $out/$name.log | sed 's/^UITK-LIVE-BEFORE //' > $out/$name.before.json
    grep -o 'UITK-LIVE-AFTER .*' $out/$name.log | sed 's/^UITK-LIVE-AFTER //' > $out/$name.after.json
    echo "$name: $(cat $out/$name.before.json) -> $(cat $out/$name.after.json)"
  }
  shim="LD_LIBRARY_PATH=${uitkFontScaleLib}/lib"
  live live-up - 130 $shim UITK_FONT_SCALE=1.00
  live live-down 130 inplace:100 $shim UITK_FONT_SCALE=1.30
  live live-start 150 - $shim UITK_FONT_SCALE=1.00
  live live-rm 150 rm $shim UITK_FONT_SCALE=1.15
  live live-bad - abc $shim UITK_FONT_SCALE=1.00
  live live-noenv 150 130 $shim
  live live-stock - 130 UITK_FONT_SCALE=1.00

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
