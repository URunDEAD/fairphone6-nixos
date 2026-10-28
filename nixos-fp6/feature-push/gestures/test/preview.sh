#!/usr/bin/env bash
# Offscreen previews + checks of Settings > Gesture Shortcuts > Back tap.
#   nix-build test -o /tmp/fp6-gestures-env
#   test/preview.sh /tmp/fp6-gestures-env OUTDIR [shot ...]
# Shots: backtap calibrating calibrated failed test checks
# Exit status: number of failed checks (FAIL lines).
set -eu
here=$(dirname "$(readlink -f "$0")")
env=$1/bin/fp6-gestures-env; out=$(readlink -f "$2"); shift 2
shots=${*:-backtap calibrating calibrated failed test checks}
mkdir -p "$out"
log=$(mktemp)
for s in $shots; do
  w=$(mktemp -d)
  cp "$here"/../settings-page/*.qml "$here/SettingsPreview.qml" "$w/"
  printf 'var shot = "%s";\nvar out = "%s";\n' "$s" "$out/$s.png" > "$w/Shot.js"
  "$env" sh -c 'QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 LANG=en_US.UTF-8 HOME=$1 timeout 60 "$FP6_QMLSCENE" "$1/SettingsPreview.qml"' _ "$w" 2>&1 \
    | grep -v "PERFORMANCE\|^$\|style override\|Available styles\|ItemPage.qml:32" | sed "s/^qml: //; s/^/[$s] /" | head -30 | tee -a "$log"
  rm -rf "$w"
done
fails=$(grep -c "FAIL " "$log" || true)
echo "checks: $(grep -c 'PASS ' "$log" || true) passed, $fails failed"
rm -f "$log"
exit "$fails"
