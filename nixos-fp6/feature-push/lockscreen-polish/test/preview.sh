#!/usr/bin/env bash
# Offscreen previews of the FP6 lock screen (1116x2484, 24 px/gu).
#   nix-build test -o /tmp/fp6-ls-env
#   test/preview.sh /tmp/fp6-ls-env OUTDIR PATCHED_LOMIRI_QML_DIR [scene ...]
# PATCHED_LOMIRI_QML_DIR: Lomiri's qml/ with all patches applied (for
# Components/Wallpaper.qml + Fp6WallpaperEffects.qml + Greeter/Fp6FingerprintHint.qml),
# e.g. from test/patchcheck.nix.
# Scenes: appear rest reading fail busy lockout success passcode quiet screenoff
#         home home-blur home-dim home-blur-dim settings
set -eu
here=$(dirname "$(readlink -f "$0")")
env=$1/bin/fp6-ls-env; out=$(readlink -f "$2"); lq=$(readlink -f "$3"); shift 3
scenes=${*:-appear rest reading fail busy lockout success passcode quiet screenoff home-blur-dim settings}
mkdir -p "$out"
wall=$("$env" sh -c 'echo $FP6_WALLPAPERS')

run_lock() { # scene shots(js) frameFrom frameTo frameStep wallpaper marker
  local s=$1 w; w=$(mktemp -d)
  cp "$here/LockPreview.qml" "$lq/Greeter/Fp6FingerprintHint.qml" "$lq/Components/Wallpaper.qml" \
     "$lq/Components/Fp6WallpaperEffects.qml" "$w/"
  mkdir -p "$out/frames/$s"
  [ "$5" -gt 0 ] || rmdir "$out/frames/$s"
  cat > "$w/Shot.js" <<JS
var shot = "$s"; var out = "$out"; var shots = $2;
var frameFrom = $3; var frameTo = $4; var frameStep = $5;
var wallpaper = "$6"; var homeWallpaper = "$wall/Infinite-Sea_by_Aury88.jpg"; var marker = ${7:-false};
JS
  "$env" sh -c 'QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 LANG=en_US.UTF-8 HOME=$1 timeout 90 "$FP6_QMLSCENE" "$1/LockPreview.qml"' _ "$w" 2>&1 \
    | grep -v "^$\|PERFORMANCE\|style override\|Available styles" | sed "s/^/[$s] /" | head -20
  rm -rf "$w"
}

W="$wall/Fossa_by_Jasper_Roks.jpg"
for s in $scenes; do
  case $s in
    appear)    run_lock appear    '[[440,"pulse"],[1300,"cue"]]' 0 3200 80 "$W" ;;
    rest)      run_lock rest      '[[3600,"idle"]]' 0 0 0 "$W" true ;;
    reading)   run_lock reading   '[[2720,"flash"]]' 2600 3100 40 "$W" ;;
    fail)      run_lock fail      '[[2680,"shake"],[3100,"text"]]' 2600 4300 40 "$W" ;;
    busy)      run_lock busy      '[[2900,"text"]]' 0 0 0 "$W" ;;
    lockout)   run_lock lockout   '[[3000,"text"],[4400,"passcode"]]' 2600 4300 50 "$W" ;;
    success)   run_lock success   '[[2700,"flare"],[2800,"ripple"]]' 2600 3100 25 "$W" ;;
    passcode)  run_lock passcode  '[[2500,"idle"],[2900,"fail"]]' 0 0 0 "$W" ;;
    quiet)     run_lock quiet     '[[3600,"idle"]]' 0 0 0 "$W" ;;
    screenoff) run_lock screenoff '[[1200,"off"]]' 0 0 0 "$W" ;;
    home|home-blur|home-dim|home-blur-dim) run_lock "$s" '[[1500,"shot"]]' 0 0 0 "$W" ;;
    settings)
      w=$(mktemp -d); gdd=$(mktemp -d)
      cp "$here/SettingsPreview.qml" "$w/"
      mkdir -p "$w/background" "$w/stubs/Lomiri/SystemSettings/Background/Theme"
      # Stub of the C++ LomiriBackgroundPanel (AccountsService) with the bundled set.
      art=$(for f in "$wall"/*; do printf '"file://%s",' "$f"; done)
      cat > "$w/stubs/Lomiri/SystemSettings/Background/qmldir" <<Q
module Lomiri.SystemSettings.Background
LomiriBackgroundPanel 1.0 LomiriBackgroundPanel.qml
Q
      cat > "$w/stubs/Lomiri/SystemSettings/Background/LomiriBackgroundPanel.qml" <<Q
import QtQuick 2.12
QtObject {
    property string backgroundFile: "file://$wall/Infinite-Sea_by_Aury88.jpg"
    property var systemArt: [ ${art%,} ]
    property var customBackgrounds: []
    property string defaultBackgroundFile: "/usr/share/backgrounds/lomiri-default-background.png"
    function prepareBackgroundFile(u, share) { return u; }
    function fileExists(f) { return true; }
    function rmFile(f) { }
}
Q
      printf 'module Lomiri.SystemSettings.Background.Theme\nLomiriThemeSettings 1.0 LomiriThemeSettings.qml\n' > "$w/stubs/Lomiri/SystemSettings/Background/Theme/qmldir"
      printf 'import QtQuick 2.12\nQtObject { property bool darkModeEnabled: false }\n' > "$w/stubs/Lomiri/SystemSettings/Background/Theme/LomiriThemeSettings.qml"
      "$env" sh -c 'cp -r $FP6_LSS_BACKGROUND/* "$1/background/"; cp $FP6_PLUGIN_QML/* "$1/background/"' _ "$w"
      chmod -R u+w "$w"
      patch -s -d "$w/background" -p1 < "$here/../wallpapers/lss-background-wallpapers.patch"
      sed -i "s|@FP6_DEFAULT_WALLPAPER@|$wall/lomiri-default-background.png|" "$w/background/MainPage.qml"
      for v in settings settings-separate settings-scrolled settings-dialog; do
        rm -f "$gdd"/fp6-lock-wallpaper-*
        [ $v = settings-separate ] || [ $v = settings-scrolled ] && cp "$wall/Kleiber_by_Lukas_Baubkus.jpg" "$gdd/fp6-lock-wallpaper-1000.jpg"
        printf 'var shot = "%s";\nvar out = "%s";\nvar pick = "%s";\n' "$v" "$out/$v.png" "$wall/Fossa_by_Jasper_Roks.jpg" > "$w/Shot.js"
        "$env" sh -c 'export QML2_IMPORT_PATH=$1/stubs:$FP6_LSS_QML:$QML2_IMPORT_PATH; XDG_GREETER_DATA_DIR=$2 QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 HOME=$1 timeout 60 "$FP6_QMLSCENE" "$1/SettingsPreview.qml"' _ "$w" "$gdd" 2>&1 \
          | grep -v "PERFORMANCE\|^$\|style override\|Available styles" | sed "s/^/[$v] /" | head -20
      done
      rm -rf "$w" "$gdd" ;;
  esac
done
