#!/usr/bin/env bash
# Laptop previews of the FP6 accessibility module (x86, xvfb + Mesa).
#   nix-build test -o $SCRATCH/a11y-env
#   test/preview.sh $SCRATCH/a11y-env OUTDIR [PATCHED_LOMIRI_TREE]
# PATCHED_LOMIRI_TREE (e.g. from ../spread-close-all/test/patched-src.nix)
# gives the quick-settings preview its fully patched QsBackend.qml.
set -eu
here=$(dirname "$(readlink -f "$0")")
env=$1/bin/fp6-a11y-env; out=$(readlink -f "$2"); tree=${3:-}
mkdir -p "$out"
run_gl() {  # scene-dir scene.qml shot
  printf 'var shot = "%s";\nvar out = "%s";\n' "$3" "$out/$3.png" > "$1/Shot.js"
  "$env" sh -c 'export QML2_IMPORT_PATH=$FP6_LSS_QML:$QML2_IMPORT_PATH; QT_QPA_PLATFORM=xcb GRID_UNIT_PX=24 LANG=en_US.UTF-8 HOME=$1 \
     timeout 90 "$FP6_XVFB_RUN" -a -s "-screen 0 1200x2600x24" "$FP6_PREVIEWER" "$1/$2" "$3" 3500' _ "$1" "$2" "$out/$3.png" 2>&1 \
     | grep -v "^$\|PERFORMANCE\|style override\|Available styles\|Fontconfig" | sed "s/^/[$3] /" | head -20
}
# whole-screen effects
w=$(mktemp -d); trap 'rm -rf "$w"' EXIT
cp "$here"/../settings-page/*.qml "$here/ScreenPreview.qml" "$w/"
for s in ${SCREEN_SCENES:-normal magnified inversion deuteranomaly protanomaly tritanomaly greyscale inversion-nightlight sim-deuteranomaly-before sim-deuteranomaly-after sim-protanomaly-before sim-protanomaly-after sim-tritanomaly-before sim-tritanomaly-after}; do
  run_gl "$w" ScreenPreview.qml "$s"
done
# Settings pages
s2=$(mktemp -d)
cp "$here"/../settings-page/*.qml "$here/SettingsPreview.qml" "$s2/"
for s in ${SETTINGS_SCENES:-settings settings-on settings-dark settings-magnification settings-colour}; do
  printf 'var shot = "%s";\nvar out = "%s";\n' "$s" "$out/$s.png" > "$s2/Shot.js"
  "$env" sh -c 'export QML2_IMPORT_PATH=$FP6_LSS_QML:$QML2_IMPORT_PATH; QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 LANG=en_US.UTF-8 HOME=$1 timeout 60 "$FP6_QMLSCENE" "$1/SettingsPreview.qml"' _ "$s2" 2>&1 \
    | grep -v "PERFORMANCE\|^$\|style override\|Available styles\|Fontconfig" | sed "s/^/[$s] /" | head -20
done
rm -rf "$s2"
# Quick settings tiles (quick-settings' harness + the patched backend)
if [ -n "$tree" ]; then
  q=$(mktemp -d); qs=$here/../../quick-settings/test
  mkdir -p "$q/Panel/QuickSettings" "$q/Panel/Indicators"
  cp "$tree"/qml/Panel/QuickSettings/* "$q/Panel/QuickSettings/"
  cp "$qs"/stubs-panel/Indicators/* "$q/Panel/Indicators/"
  cp "$qs/Main.qml" "$here/QsPreview.qml" "$q/"; chmod -R u+w "$q"
  for s in qs-off qs-on qs-toggletest; do
    printf 'var shot = "%s";\nvar out = "%s";\n' "$s" "$out/$s.png" > "$q/Shot.js"
    "$env" sh -c 'QT_QPA_PLATFORM=offscreen QT_STYLE_OVERRIDE= GRID_UNIT_PX=24 LANG=en_US.UTF-8 HOME=$1 timeout 60 "$FP6_QMLSCENE" "$1/QsPreview.qml"' _ "$q" 2>&1 \
      | grep -v "^$\|PERFORMANCE\|Fontconfig\|browser-app" | sed "s/^/[$s] /" | head -60
  done
  rm -rf "$q"
fi
ls -la "$out"/*.png | wc -l
