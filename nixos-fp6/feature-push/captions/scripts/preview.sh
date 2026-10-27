#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d /tmp/fp6-captions-preview.XXXXXX)
trap 'rm -rf "$work"' EXIT
export XDG_CONFIG_HOME="$work/config" XDG_CACHE_HOME="$work/cache"
export QT_QPA_PLATFORM=offscreen LIBGL_ALWAYS_SOFTWARE=1
unset QT_QUICK_BACKEND QT_STYLE_OVERRIDE QT_QPA_PLATFORMTHEME
mkdir -p "$repo/previews" "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME"
binary=${CAPTIONS_UI:-${CARGO_TARGET_DIR:-/tmp/fp6-captions-target}/debug/fp6-captions-ui}
timeout 30s xvfb-run -a "$binary" --preview="$repo/previews/captions-en.png" --language=en
timeout 30s xvfb-run -a "$binary" --preview="$repo/previews/captions-ro.png" --language=ro
timeout 30s xvfb-run -a "$binary" --preview="$repo/previews/captions-collapsed.png" --collapsed=true
timeout 30s xvfb-run -a "$binary" --preview="$repo/previews/captions-settings.png" --settings=true
for name in captions-en captions-ro captions-collapsed captions-settings; do test -s "$repo/previews/$name.png"; done
