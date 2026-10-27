#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export QT_QPA_PLATFORM=offscreen
unset QT_QUICK_BACKEND QT_QPA_OFFSCREEN_NO_GLX
export LIBGL_ALWAYS_SOFTWARE=1
export GALLIUM_DRIVER=llvmpipe
export QSG_RENDER_LOOP=basic
export QT_STYLE_OVERRIDE=Fusion
if test -z "${LIBGL_DRIVERS_PATH:-}" && test -d /run/opengl-driver/lib/dri; then
    export LIBGL_DRIVERS_PATH=/run/opengl-driver/lib/dri
fi
export QT_OPENGL=software
if test -z "${DISPLAY:-}"; then
    exec xvfb-run -a -s "-screen 0 1024x1024x24" "$0" "$@"
fi
export GRID_UNIT_PX=8
export XDG_CACHE_HOME="${TMPDIR:-/tmp}/lomiri-kdeconnect-preview-cache"
mkdir -p previews "$XDG_CACHE_HOME"
# The package wrapper supplies all toolkit imports and the installed QML path.
program="${KDECONNECT_PREVIEW_BINARY:-./result/bin/lomiri-kdeconnect}"
for page in devices plugins pairing; do
    timeout 45 "$program" --preview "$PWD/previews/$page.png" --page "$page"
    test -s "previews/$page.png"
done
