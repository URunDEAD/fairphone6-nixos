#!/usr/bin/env bash
# Text size layout checks on the laptop (x86, offscreen, software Qt Quick):
#   run.sh OUTDIR [DIALER_QML_DIR]
# Drawer cells + panel (DrawerLayout.qml), Phone call-history and Messages
# thread rows (AppRows.qml), stock and patched geometry, at 100/115/130/150 %;
# the installed Phone keypad (Keypad.qml, KeypadButton.qml copied from
# DIALER_QML_DIR/DialerPage, LomiriShape -> Rectangle because LomiriShape
# crashes the software renderer). Prints one LAYOUT-RESULT per run.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
out=$(realpath -m "$1"); dialer=${2:-}
mkdir -p "$out"
lib=$(nix-build --no-out-link "$here/x86.nix" -A fontScaleLib)
qs=$(nix-build --no-out-link "$here/x86.nix" -A qmlscene)
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
cp "$here"/*.qml "$work/"
if [ -n "$dialer" ]; then
  mkdir "$work/dialer"
  cp "$dialer"/DialerPage/{Keypad,KeypadButton}.qml "$work/dialer/"
  chmod u+w "$work"/dialer/*.qml
  sed -i 's/^    LomiriShape {/    Rectangle {/' "$work/dialer/KeypadButton.qml"
fi
run() { # NAME FILE SCALE
  d=$(mktemp -d -p "$work")
  ( cd "$d" && env -u DISPLAY -u WAYLAND_DISPLAY HOME="$d" XDG_CONFIG_HOME="$d" XDG_RUNTIME_DIR="$d" \
      QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software GRID_UNIT_PX=24 \
      UITK_FONT_SCALE="$3" LD_LIBRARY_PATH="$lib/lib" timeout 120 "$qs" "$work/$2" ) > "$out/$1.log" 2>&1 || true
  cp "$d/grab.png" "$out/$1.png" 2>/dev/null || true
  echo "$1: $(grep -o 'LAYOUT-RESULT.*' "$out/$1.log" | sed 's/^LAYOUT-RESULT //' || echo FAILED)"
}
for s in 1.00 1.15 1.30 1.50; do
  for v in stock patched; do
    run drawer-$v-$s drawer-$v.qml $s
    run rows-$v-$s rows-$v.qml $s
  done
  [ -z "$dialer" ] || run keypad-$s keypad.qml $s
done
