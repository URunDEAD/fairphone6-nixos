#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p previews "$work/runtime" "$work/data" "$work/config" "$work/cache"
chmod 700 "$work/runtime"
export QT_QPA_PLATFORM=offscreen QT_SCALE_FACTOR=1 GRID_UNIT_PX=8
export XDG_RUNTIME_DIR="$work/runtime" XDG_DATA_HOME="$work/data" XDG_CONFIG_HOME="$work/config" XDG_CACHE_HOME="$work/cache"
export LIBGL_ALWAYS_SOFTWARE=1 GSETTINGS_BACKEND=memory QT_STYLE_OVERRIDE=Fusion
unset QT_QUICK_BACKEND QT_QPA_OFFSCREEN_NO_GLX
binary=${FP6_SHARE_UI_BIN:-./target/debug/fp6-share-ui}
for mode in image text empty error; do
  xvfb-run -a dbus-run-session -- timeout 40 "$binary" --preview "$mode" --screenshot "$PWD/previews/$mode.png"
  test -s "previews/$mode.png"
done
