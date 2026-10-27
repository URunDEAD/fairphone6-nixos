#!/usr/bin/env bash
# Smoke test of the patched Lomiri panel on the laptop (offscreen).
#   panel-smoke.sh LOMIRI_X86 QS_ENV OUTDIR
# LOMIRI_X86: an x86 build of the target's patched Lomiri (its share/lomiri
# QML and the test mocks it installs); QS_ENV: nix-build test.
set -eu
here=$(dirname "$(readlink -f "$0")")
L=$(readlink -f "$1"); env=$2/bin/fp6-qs-env; out=$(readlink -f "$3")
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
cp -r "$L"/share/lomiri/. "$work/"; chmod -R u+w "$work"
mkdir -p "$work/stubs/QtMir/Application" "$out"
printf 'module QtMir.Application\nFp6Dummy 0.1 Fp6Dummy.qml\n' > "$work/stubs/QtMir/Application/qmldir"; printf 'import QtQuick 2.15\nQtObject {}\n' > "$work/stubs/QtMir/Application/Fp6Dummy.qml"
cp "$here/PanelSmoke.qml" "$work/"
printf 'var out = "%s";\n' "$out" > "$work/Shot.js"
"$env" sh -c '
  real=$(echo "$QML2_IMPORT_PATH" | tr : "\n" | grep -v "/stubs$" | paste -sd:)
  gs=$(echo "$real" | tr : "\n" | grep gsettings-qt | head -1)
  export QML2_IMPORT_PATH="$1/stubs:$gs:$2/lib/qt-5.15.19/qml/mocks:$2/lib/qt-5.15.19/qml:$real"
  export LD_LIBRARY_PATH=$2/lib:$2/lib/qt-5.15.19/qml/mocks/liblightdm:$2/lib/qt-5.15.19/qml/mocks/libusermetrics
  QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 HOME=$1 timeout 90 "$FP6_QMLSCENE" "$1/PanelSmoke.qml"' _ "$work" "$L" 2>&1 \
  | grep -v "PERFORMANCE\|image://theme" | head -80
