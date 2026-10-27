#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p previews
export QT_QPA_PLATFORM=offscreen
export QT_QUICK_BACKEND=software
export QT_STYLE_OVERRIDE=Fusion
export QT_QPA_OFFSCREEN_NO_GLX=1
export QSG_RENDER_LOOP=basic
export GRID_UNIT_PX=8
export XDG_RUNTIME_DIR
XDG_RUNTIME_DIR=$(mktemp -d /tmp/fp6-qml.XXXXXX)
trap 'rm -rf "$XDG_RUNTIME_DIR"' EXIT
for scenario in incoming silent waiting active held; do
    args=()
    case "$scenario" in
        silent) args=(--silent);;
        waiting) args=(--waiting);;
        active) args=(--active);;
        held) args=(--held);;
    esac
    timeout 20s qmlscene -platform offscreen qml/Preview.qml "${args[@]}" "--capture=$PWD/previews/$scenario.png"
done
