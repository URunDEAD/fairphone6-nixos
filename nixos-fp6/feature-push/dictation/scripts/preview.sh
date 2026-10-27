#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
preview_dir=$(mktemp -d)
trap 'rm -rf "$preview_dir"' EXIT
export XDG_CACHE_HOME="$preview_dir/cache"
mkdir -p "$XDG_CACHE_HOME"
export QT_QPA_PLATFORM=offscreen LIBGL_ALWAYS_SOFTWARE=1
unset QT_QUICK_BACKEND
unset QT_STYLE_OVERRIDE QT_QPA_PLATFORMTHEME
mkdir -p "$repo/previews"
xvfb-run -a qmlscene "$repo/qml/Settings.qml" "--config=$preview_dir/settings.ini" "--preview=$repo/previews/settings-en.png"
printf '[General]\nlanguage=ro\nmodel=base-q5_1\npunctuation=false\n' > "$preview_dir/settings.ini"
xvfb-run -a qmlscene "$repo/qml/Settings.qml" "--config=$preview_dir/settings.ini" "--preview=$repo/previews/settings-ro.png"
