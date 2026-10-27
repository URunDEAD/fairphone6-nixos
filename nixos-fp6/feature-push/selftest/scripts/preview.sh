#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d /tmp/fp6-selftest-preview.XXXXXX)
trap 'rm -rf "$work"' EXIT
mkdir -p previews "$work/runtime" "$work/data" "$work/config" "$work/cache"
chmod 700 "$work/runtime"
export QT_QPA_PLATFORM=offscreen QT_SCALE_FACTOR=1 GRID_UNIT_PX=8
export XDG_RUNTIME_DIR="$work/runtime" XDG_DATA_HOME="$work/data" XDG_CONFIG_HOME="$work/config" XDG_CACHE_HOME="$work/cache"
export LIBGL_ALWAYS_SOFTWARE=1 GSETTINGS_BACKEND=memory QT_STYLE_OVERRIDE=Fusion QT_QPA_PLATFORMTHEME=
export QML_DISABLE_DISK_CACHE=1
unset QT_QUICK_BACKEND QT_QPA_OFFSCREEN_NO_GLX
if [[ -z ${FP6_SELFTEST_UI_BIN:-} ]]; then
  cargo build --features ui
fi
binary=${FP6_SELFTEST_UI_BIN:-${CARGO_TARGET_DIR:-target}/debug/fp6-selftest-ui}
xvfb-run -a dbus-run-session -- timeout 30 "$binary" --preview "$PWD/previews/ready.png"
xvfb-run -a dbus-run-session -- timeout 30 "$binary" --preview "$PWD/previews/results.png" --results
test -s previews/ready.png
test -s previews/results.png
