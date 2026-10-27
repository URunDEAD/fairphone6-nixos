#!/usr/bin/env bash
# Offscreen laptop test + previews of the spread close-all.
#   nix-build test -o $SCRATCH/sca-env
#   test/run-tests.sh $SCRATCH/sca-env LOMIRI_X86 PATCHED_TREE OUTDIR
# LOMIRI_X86: any x86 Lomiri 0.5 build (only its Lomiri.Gestures / Utils QML
# plugins are used); PATCHED_TREE: the target's patched Lomiri source with
# this module's patch (test/patched-src.nix); OUTDIR: screenshots + log.
set -eu
here=$(dirname "$(readlink -f "$0")")
env=$1/bin/fp6-sca-env; L=$(readlink -f "$2"); tree=$(readlink -f "$3"); out=$(readlink -f "$4")
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
mkdir -p "$out"
cp -r "$tree"/qml/. "$work/"; chmod -R u+w "$work"
cp "$here/tst_SpreadCloseAll.qml" "$work/"
cp "$here/../qml/Fp6CloseAll.qml" "$work/Stage/Spread/"   # the working copy
printf 'var out = "%s";\n' "$out" > "$work/Shot.js"
"$env" sh -c '
  export QML2_IMPORT_PATH="$QML2_IMPORT_PATH:$2/lib/qt-5.15.19/qml"
  QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 LANG=en_US.UTF-8 HOME=$1 timeout 300 "$FP6_QMLTESTRUNNER" -input "$1/tst_SpreadCloseAll.qml"' _ "$work" "$L" 2>&1 \
  | grep -v "PERFORMANCE\|image://theme\|^$" | tee "$out/test-log.txt"
# Settings > Gesture Shortcuts (the real page, App switcher section on top)
w=$(mktemp -d)
cp "$here"/../../gestures/settings-page/*.qml "$here/SettingsPreview.qml" "$w/"
for s in settings settings-off; do
  printf 'var shot = "%s";\nvar out = "%s";\n' "$s" "$out/$s.png" > "$w/Shot.js"
  "$env" sh -c 'export QML2_IMPORT_PATH=$FP6_LSS_QML:$QML2_IMPORT_PATH; QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 HOME=$1 timeout 60 "$FP6_QMLSCENE" "$1/SettingsPreview.qml"' _ "$w" 2>&1 \
    | grep -v "PERFORMANCE\|^$\|style override\|Available styles" | sed "s/^/[$s] /" | head -20
done
rm -rf "$w"
