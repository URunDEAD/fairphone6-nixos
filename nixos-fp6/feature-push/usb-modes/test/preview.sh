#!/usr/bin/env bash
# Laptop previews of System Settings > USB and the agent's notifications
# (offscreen, 1116x2484, 24 px/gu).
#   nix-build test -o /tmp/fp6-usbmodes-env
#   test/preview.sh /tmp/fp6-usbmodes-env OUTDIR
set -eu
here=$(dirname "$(readlink -f "$0")")
env=$1/bin/fp6-usbmodes-env; out=$(readlink -f "$2")
mkdir -p "$out"
shot() { # name kind status mode scroll dark summary body actions
  local w; w=$(mktemp -d)
  cp "$here"/../settings-page/PageComponent.qml "$here/Preview.qml" "$w/"
  cat > "$w/Shot.js" <<JS
var out = "$out/$1.png"; var kind = "$2"; var status = "file://$here/scenes/$3.json";
var mode = "$4"; var scroll = ${5:-0}; var dark = ${6:-false};
var summary = "${7:-}"; var body = "${8:-}"; var actions = [${9:-}];
JS
  "$env" sh -c 'QML_XHR_ALLOW_FILE_READ=1 QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 HOME=$1 timeout 60 "$FP6_QMLSCENE" "$1/Preview.qml"' _ "$w" 2>&1 \
    | grep -v "PERFORMANCE\|^$\|style override\|Available styles" | sed "s/^/[$1] /" | head -20
  rm -rf "$w"
}
shot 01-page-dev-laptop page dev-laptop system 0
shot 02-page-dev-laptop-scrolled page dev-laptop system 55
shot 03-page-user-charge-only page user-charge-only charge 0
shot 04-page-mtp-adb page user-mtp-adb last 0 true
shot 05-page-reverse-charging page pd-phone-source ask 0
shot 06-page-not-connected page not-connected system 0
shot 07-notification-connected notification none "" 0 false "Charging via USB" "USB network · Tap for options" ''
shot 08-notification-ask notification none "" 0 false "Use USB for" "A computer is connected. Choose what it may use; the phone charges either way." '"Charge only","File transfer","USB network","More options…"'
