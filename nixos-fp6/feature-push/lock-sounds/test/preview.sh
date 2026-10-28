#!/usr/bin/env bash
# Offscreen previews (1116x2484) + checks of Settings > Sound > Lock sounds,
# inside fp6-ls-env (test/run-tests.sh does): the Sound page as the target
# builds it and the real picker pages. Scenes:
#   sound sound-custom sound-off picker picker-custom files files-music files-choose
# Exit status: number of failed checks.
set -eu
here=$(dirname "$(readlink -f "$0")")
out=$(readlink -f "$1"); shift
scenes=${*:-sound sound-custom sound-off picker picker-custom files files-music files-choose}
log=$(mktemp)
# Lomiri.Connectivity (indicator-network) is not built for x86 here: a stub.
stubs=$(mktemp -d)
mkdir -p "$stubs/Lomiri/Connectivity"
printf 'module Lomiri.Connectivity\nsingleton NetworkingStatus 1.0 NetworkingStatus.qml\n' > "$stubs/Lomiri/Connectivity/qmldir"
printf 'pragma Singleton\nimport QtQuick 2.12\nQtObject { property bool modemAvailable: true }\n' > "$stubs/Lomiri/Connectivity/NetworkingStatus.qml"
for s in $scenes; do
  w=$(mktemp -d)
  cp "$FP6_LS_LSS"/* "$here/SettingsPreview.qml" "$w/"
  chmod -R u+w "$w"
  mkdir -p "$w/home/Music/Old" "$w/home/Ringtones"
  # two short files from the bundle under new names
  cp "$FP6_LS_BUNDLE/tap.wav" "$w/home/Music/A soft tap.wav"
  cp "$FP6_LS_RINGTONE" "$w/home/Music/Quiet bell.ogg"
  printf 'var shot = "%s";\nvar out = "%s";\nvar home = "%s";\n' "$s" "$out/settings-$s.png" "$w/home" > "$w/Shot.js"
  QML2_IMPORT_PATH=$stubs:$FP6_LSS_QML:$QML2_IMPORT_PATH QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 HOME=$w/home \
    timeout 60 "$FP6_QMLSCENE" "$w/SettingsPreview.qml" 2>&1 \
    | grep -v "PERFORMANCE\|^$\|style override\|Available styles\|QDBus\|AccountsService\|org.freedesktop" \
    | sed "s/^qml: //; s/^/[$s] /" | head -30 | tee -a "$log"
  rm -rf "$w"
done
rm -rf "$stubs"
fails=$(grep -c "FAIL " "$log" || true)
echo "checks: $(grep -c 'PASS ' "$log" || true) passed, $fails failed"
rm -f "$log"
exit "$fails"
