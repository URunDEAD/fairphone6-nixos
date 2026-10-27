#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
app=${FP6_APP:-$repo/result/bin/fp6-quickshare-app}
out=${1:-/tmp/fp6-quickshare-previews}
mkdir -p "$out/cache"
# UITK's ShapeMaterial requires OpenGL, including in offscreen mode.
unset QT_QUICK_BACKEND
export QT_QPA_PLATFORM=offscreen LIBGL_ALWAYS_SOFTWARE=1 XDG_CACHE_HOME="$out/cache"
"$app" --preview --screenshot "$out/receive.png"
"$app" --preview-send --screenshot "$out/send.png"
test -s "$out/receive.png"
test -s "$out/send.png"
printf 'Previews saved in %s\n' "$out"
