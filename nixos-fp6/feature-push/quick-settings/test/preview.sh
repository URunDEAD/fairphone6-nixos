#!/usr/bin/env bash
# Laptop preview of the FP6 control centre (offscreen, 1116x2484, 24 px/gu).
#   nix-build test -o /tmp/fp6-qs-env
#   test/preview.sh /tmp/fp6-qs-env OUTDIR [scene ...]
# Scenes: dark, light, expanded, locked, edit, edit-light, w5, w6-expanded
set -eu
here=$(dirname "$(readlink -f "$0")")
env=$1/bin/fp6-qs-env; out=$(readlink -f "$2"); shift 2
scenes=${*:-dark light expanded locked edit w6-expanded}
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
mkdir -p "$work/Panel/QuickSettings" "$work/Panel/Indicators" "$out"
cp "$here"/../qml/* "$work/Panel/QuickSettings/"
cp "$here"/stubs-panel/Indicators/* "$work/Panel/Indicators/"
cp "$here/Main.qml" "$work/"
for s in $scenes; do
  printf 'var shot = "%s";\nvar out = "%s";\n' "$s" "$out/$s.png" > "$work/Shot.js"
  "$env" sh -c 'QT_QPA_PLATFORM=offscreen QT_STYLE_OVERRIDE= GRID_UNIT_PX=24 LANG=en_US.UTF-8 HOME=$1 timeout 60 "$FP6_QMLSCENE" "$1/Main.qml"' _ \
    "$work" 2>&1 | grep -v "^$" | sed "s/^/[$s] /" | head -40
done
# System Settings page
for s in ${SETTINGS_SCENES:-settings settings-scrolled}; do
  w=$(mktemp -d)
  cp "$here"/../settings-page/*.qml "$here/SettingsPreview.qml" "$w/"
  printf 'var shot = "%s";\nvar out = "%s";\n' "$s" "$out/$s.png" > "$w/Shot.js"
  "$env" sh -c 'export QML2_IMPORT_PATH=$FP6_LSS_QML:$QML2_IMPORT_PATH; QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 HOME=$1 timeout 60 "$FP6_QMLSCENE" "$1/SettingsPreview.qml"' _ "$w" 2>&1 \
    | grep -v "PERFORMANCE\|^$\|style override\|Available styles" | sed "s/^/[$s] /" | head -20
  rm -rf "$w"
done
