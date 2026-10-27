#!/usr/bin/env bash
# Laptop (x86) previews of the call-recording dialer UI, offscreen.
#   nix-build test/qml -o $TMP/fp6-callrec-qml
#   test/qml/preview.sh $TMP/fp6-callrec-qml OUTDIR [scene ...]
# The pinned dialer source gets lomiri-dialer-app-call-recording.patch and the
# new pages (as in dialer/dialer.nix); Main.qml loads the scene's page.
set -eu
here=$(dirname "$(readlink -f "$0")")
dialer=$here/../../dialer
env=$1/bin/fp6-callrec-qml; mkdir -p "$2"; out=$(readlink -f "$2"); shift 2
scenes=${*:-incall-idle incall-recording incall-fallback incall-keypad-recording incall-wide-recording incall-notice settings-off settings-auto-contacts notice recordings recordings-expanded recordings-noservice phone-settings}
src=$("$env" sh -c 'echo $FP6_DIALER_SRC')
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
cp -r "$src/src" "$work/src"; chmod -R u+w "$work"
patch -s -p1 -d "$work" -i "$dialer/lomiri-dialer-app-call-recording.patch"
cp "$dialer"/CallRecorder.qml "$dialer"/CallRecordingNotice.qml "$work/src/qml/"
cp "$dialer"/CallRecordingSettings.qml "$dialer"/CallRecordingContactPicker.qml "$dialer"/RecordingsPage.qml "$work/src/qml/SettingsPage/"
ln -s src/qml "$work/qml"
cp "$here/Main.qml" "$work/"
for s in $scenes; do
  w=1116; h=2412
  case $s in *wide*) w=2412; h=1116;; esac
  dark=false; case $s in *dark*) dark=true;; esac
  printf 'var shot = "%s";\nvar out = "%s";\nvar width = %s;\nvar height = %s;\nvar dark = %s;\n' \
    "${s%-dark}" "$out/qml-$s.png" $w $h $dark > "$work/Shot.js"
  "$env" sh -c 'QT_QPA_PLATFORM=offscreen QT_STYLE_OVERRIDE= GRID_UNIT_PX=24 LANG=en_US.UTF-8 HOME=$1 XDG_CONFIG_HOME=$1/.config timeout 60 "$FP6_QMLSCENE" "$1/Main.qml"' _ \
    "$work" 2>&1 | grep -v "^$\|PERFORMANCE\|style override\|Available styles" | sed "s/^/[$s] /" | head -40
done
