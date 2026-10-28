#!/usr/bin/env bash
# Offscreen previews + behaviour checks of FP6 app permissions (1116x2484, 24 px/gu).
#   nix-build test -o /tmp/fp6-perm-env
#   nix-build test/patchcheck.nix -o /tmp/fp6-lomiri-perm
#   test/preview.sh /tmp/fp6-perm-env OUTDIR /tmp/fp6-lomiri-perm [scene ...]
# The prompt QML comes from the patched Lomiri tree; the settings scenes use
# the stock AppAccess.qml patched with settings-page/lss-app-access.patch.
# Exit status: number of FAIL lines.
set -eu
here=$(dirname "$(readlink -f "$0")")
env=$1/bin/fp6-perm-env; out=$(readlink -f "$2"); lq=$(readlink -f "$3"); shift 3
all="prompt prompt-locked settings-main settings-perm settings-app settings-app-nonet settings-app-system
     settings-network settings-history settings-decision"
scenes=${*:-$all}
mkdir -p "$out"
log=$(mktemp)
wall=$("$env" sh -c 'echo $FP6_WALLPAPERS')/Fossa_by_Jasper_Roks.jpg
for s in $scenes; do
  w=$(mktemp -d)
  cp "$here/Sample.js" "$w/"
  printf 'var shot = "%s";\nvar out = "%s";\nvar wallpaper = "%s";\n' "$s" "$out" "$wall" > "$w/Shot.js"
  case $s in
    settings*)
      mkdir -p "$w/security-privacy" "$w/fp6-app-permissions"
      cp "$here"/../settings-page/*.qml "$w/fp6-app-permissions/"
      sp=$("$env" sh -c 'echo $FP6_LSS_SP')
      cp --no-preserve=mode "$sp"/*.qml "$w/security-privacy/"
      patch -s -d "$w/security-privacy" -p1 < "$here/../settings-page/lss-app-access.patch"
      cp "$here/SettingsPreview.qml" "$w/"
      main=SettingsPreview.qml
      imp='export QML2_IMPORT_PATH=$FP6_LSS_QML:$QML2_IMPORT_PATH;'
      ;;
    *)
      mkdir -p "$w/Components"
      cp "$lq/qml/Components/Fp6PermissionPrompt.qml" "$w/Components/"
      cp "$here/PromptPreview.qml" "$w/"
      main=PromptPreview.qml
      imp=
      ;;
  esac
  "$env" sh -c "$imp"' QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 LANG=en_US.UTF-8 HOME=$1 timeout 60 "$FP6_QMLSCENE" "$1/$2"' _ "$w" "$main" 2>&1 \
    | grep -v "^$\|PERFORMANCE\|style override\|Available styles\|qt.svg\|ACTION " | sed "s/^qml: //; s/^/[$s] /" | head -40 | tee -a "$log"
  if [ -n "${KEEP:-}" ]; then echo "kept $w"; else rm -rf "$w"; fi
done
fails=$(grep -c "FAIL " "$log" || true)
echo "checks: $(grep -c 'PASS ' "$log" || true) passed, $fails failed"
rm -f "$log"
exit "$fails"
