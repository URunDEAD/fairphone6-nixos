#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export QT_QPA_PLATFORM=offscreen
unset QT_QUICK_BACKEND
export LIBGL_ALWAYS_SOFTWARE=1
export QSG_RENDER_LOOP=basic
export QT_STYLE_OVERRIDE=Fusion
ulimit -c 0
mkdir -p previews
app=${1:-target/debug/fp6-bugreport-gui}
# The offscreen platform uses GLX for UITK shape nodes. Supply a private display
# when running on a headless machine; all windows still use the offscreen QPA.
if [[ -z "${DISPLAY:-}" ]]; then
  exec xvfb-run -a "$0" "$app"
fi
export XDG_RUNTIME_DIR
XDG_RUNTIME_DIR=$(mktemp -d)
trap 'rm -rf "$XDG_RUNTIME_DIR"' EXIT
chmod 700 "$XDG_RUNTIME_DIR"
timeout 30 "$app" --preview "$PWD/previews/overview.png"
timeout 30 "$app" --preview "$PWD/previews/sections.png" --preview-details
timeout 30 "$app" --preview "$PWD/previews/capture.png" --preview-capture
for path in previews/overview.png previews/sections.png previews/capture.png; do
  test -s "$path"
done
