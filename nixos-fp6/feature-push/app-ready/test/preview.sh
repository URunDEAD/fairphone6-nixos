#!/usr/bin/env bash
# Offscreen previews (1116x2484) + checks of Settings > Memory & performance,
# inside fp6-mp-env (run-tests.sh does). Exit status: number of failed checks.
set -eu
here=$(dirname "$(readlink -f "$0")")
out=$(readlink -f "$1"); shift
scenes=${*:-main main-bottom main-empty advanced advanced-bottom never-picker clear}
log=$(mktemp)
for s in $scenes; do
  w=$(mktemp -d)
  cp "$FP6_MP_PLUGIN"/*.qml "$here/SettingsPreview.qml" "$w/"
  chmod -R u+w "$w"
  sed "s/TIME/$(date +%s)/g" "$here/status.json" > "$w/status.json"
  printf 'var shot = "%s";\nvar out = "%s";\nvar status = "%s";\n' "$s" "$out/settings-$s.png" "$w/status.json" > "$w/Shot.js"
  QML2_IMPORT_PATH=$FP6_LSS_QML:$QML2_IMPORT_PATH QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 HOME=$w \
    timeout 60 "$FP6_QMLSCENE" "$w/SettingsPreview.qml" 2>&1 \
    | grep -v "PERFORMANCE\|^$\|style override\|Available styles\|QDBus\|AccountsService\|org.freedesktop" \
    | sed "s/^qml: //; s/^/[$s] /" | head -40 | tee -a "$log"
  rm -rf "$w"
done
fails=$(grep -c "FAIL " "$log" || true)
echo "checks: $(grep -c 'PASS ' "$log" || true) passed, $fails failed"
rm -f "$log"
exit "$fails"
