#!/usr/bin/env bash
# Offscreen previews + behaviour checks of FP6 charging feedback (1116x2484, 24 px/gu).
#   nix-build test -o /tmp/fp6-cf-env
#   nix-build test/patchcheck.nix -o /tmp/fp6-lomiri-cf
#   test/preview.sh /tmp/fp6-cf-env OUTDIR /tmp/fp6-lomiri-cf [scene ...]
# The QML under test is taken from the patched Lomiri tree (so the build's
# substitutions are what is previewed). Exit status: number of FAIL lines.
set -eu
here=$(dirname "$(readlink -f "$0")")
env=$1/bin/fp6-cf-env; out=$(readlink -f "$2"); lq=$(readlink -f "$3"); shift 3
all="lock-charging lock-fast lock-slow lock-limited lock-paused lock-adaptive lock-full lock-wireless
     lock-fingerprint lock-off lock-unplugged anim-locked anim-limited anim-slow anim-screenoff anim-refused
     anim-nowake anim-dnd anim-silent anim-off anim-unplug anim-bounce home-banner home-usbhost
     home-usbhost-late preview settings"
scenes=${*:-$all}
mkdir -p "$out/frames"
log=$(mktemp)
wall=$("$env" sh -c 'echo $FP6_WALLPAPERS')/Fossa_by_Jasper_Roks.jpg
for s in $scenes; do
  w=$(mktemp -d)
  mkdir -p "$w/Components" "$w/Greeter"
  case $s in
    settings*)
      cp "$here/../settings-page/Fp6ChargingFeedbackPage.qml" "$here/SettingsPreview.qml" "$w/"
      printf 'var shot = "%s";\nvar out = "%s";\n' "$s" "$out/$s.png" > "$w/Shot.js"
      "$env" sh -c 'export QML2_IMPORT_PATH=$FP6_LSS_QML:$QML2_IMPORT_PATH; QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 HOME=$1 timeout 60 "$FP6_QMLSCENE" "$1/SettingsPreview.qml"' _ "$w" 2>&1 \
        | grep -v "PERFORMANCE\|^$\|style override\|Available styles" | sed "s/^qml: //; s/^/[$s] /" | head -20 | tee -a "$log"
      ;;
    *)
      cp "$lq"/qml/Components/Fp6Charging*.qml "$w/Components/"
      cp "$here/NotificationAudio.qml" "$w/Components/"
      cp "$lq/qml/Greeter/Fp6ChargingStatus.qml" "$lq/qml/Greeter/Fp6FingerprintHint.qml" "$w/Greeter/"
      cp "$here/Preview.qml" "$w/"
      printf 'var shot = "%s";\nvar out = "%s";\nvar wallpaper = "%s";\n' "$s" "$out" "$wall" > "$w/Shot.js"
      "$env" sh -c 'QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 LANG=en_US.UTF-8 TZ=Europe/Bucharest HOME=$1 timeout 90 "$FP6_QMLSCENE" "$1/Preview.qml"' _ "$w" 2>&1 \
        | grep -v "^$\|PERFORMANCE\|style override\|Available styles\|qt.svg" | sed "s/^qml: //; s/^/[$s] /" | head -40 | tee -a "$log"
      ;;
  esac
  if [ -n "${KEEP:-}" ]; then echo "kept $w"; else rm -rf "$w"; fi
done
for a in anim-locked home-banner; do
  if ls "$out"/frames/$a-*.png >/dev/null 2>&1; then
    "$env" sh -c '"$FP6_MAGICK" -delay 7 -loop 0 "$1"/frames/'$a'-*.png "$1/'$a'.gif"' _ "$out" || true
  fi
done
fails=$(grep -c "FAIL " "$log" || true)
echo "checks: $(grep -c 'PASS ' "$log" || true) passed, $fails failed"
rm -f "$log"
exit "$fails"
