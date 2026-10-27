#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --features ui --bin netpolicy-ui
mkdir -p previews
# UITK's LomiriShape needs OpenGL. Mesa llvmpipe + Xvfb provide that even with
# QT_QPA_PLATFORM=offscreen. Qt Quick's software scenegraph crashes in UITK.
export QT_QPA_PLATFORM=offscreen LIBGL_ALWAYS_SOFTWARE=1
unset QT_QUICK_BACKEND
export QT_QPA_PLATFORMTHEME= QT_STYLE_OVERRIDE=
for page in settings usage thresholds; do
  NETPOLICY_PREVIEW_PAGE=$page timeout 30s xvfb-run -a \
    target/debug/netpolicy-ui --preview "$PWD/previews/$page.png"
  test -s "previews/$page.png"
done
