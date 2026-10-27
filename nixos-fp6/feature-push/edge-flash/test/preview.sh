#!/usr/bin/env bash
# Offscreen previews + behaviour checks of FP6 edge lighting (1116x2484).
#   nix-build test -o /tmp/fp6-edge-env
#   test/preview.sh /tmp/fp6-edge-env OUTDIR [scene ...]
# Scenes: app fixed hash policy refused wake again preview settings settings-fixed settings-off
# Exit status: number of failed checks (FAIL lines).
set -eu
here=$(dirname "$(readlink -f "$0")")
env=$1/bin/fp6-edge-env; out=$(readlink -f "$2"); shift 2
scenes=${*:-app fixed hash policy refused wake again preview settings settings-fixed settings-off}
mkdir -p "$out/frames"
log=$(mktemp)
for s in $scenes; do
  w=$(mktemp -d)
  case $s in
    settings*)
      cp "$here/../settings-page/PageComponent.qml" "$here/SettingsPreview.qml" "$w/"
      printf 'var shot = "%s";\nvar out = "%s";\n' "$s" "$out/$s.png" > "$w/Shot.js"
      "$env" sh -c 'export QML2_IMPORT_PATH=$FP6_LSS_QML:$QML2_IMPORT_PATH; QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 HOME=$1 timeout 60 "$FP6_QMLSCENE" "$1/SettingsPreview.qml"' _ "$w" 2>&1 \
        | grep -v "PERFORMANCE\|^$\|style override\|Available styles" | sed "s/^/[$s] /" | head -20 | tee -a "$log"
      ;;
    *)
      sed 's/@fp6EdgeCornerRadiusPx@/110/' "$here/../qml/Fp6EdgeFlash.qml" > "$w/Fp6EdgeFlash.qml"
      cp "$here/EdgePreview.qml" "$w/"
      printf 'var shot = "%s";\nvar out = "%s";\nvar chatIcon = "file://%s";\nvar mailIcon = "file://%s";\n' \
        "$s" "$out" "$here/chat-icon.svg" "$here/mail-icon.svg" > "$w/Shot.js"
      "$env" sh -c 'QT_QPA_PLATFORM=offscreen LANG=en_US.UTF-8 HOME=$1 timeout 90 "$FP6_QMLSCENE" "$1/EdgePreview.qml"' _ "$w" 2>&1 \
        | grep -v "^$\|PERFORMANCE\|style override\|Available styles" | sed "s/^qml: //; s/^/[$s] /" | head -40 | tee -a "$log"
      ;;
  esac
  rm -rf "$w"
done
# Animated strip of the "app" scene (quarter size).
if ls "$out"/frames/app-*.png >/dev/null 2>&1; then
  "$env" sh -c '"$FP6_MAGICK" -delay 7 -loop 0 "$1"/frames/app-*.png "$1/app-animation.gif"' _ "$out" || true
fi
fails=$(grep -c "FAIL " "$log" || true)
echo "checks: $(grep -c 'PASS ' "$log" || true) passed, $fails failed"
rm -f "$log"
exit "$fails"
