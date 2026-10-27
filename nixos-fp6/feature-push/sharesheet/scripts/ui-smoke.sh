#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/runtime" "$work/data" "$work/cache" "$work/config" "$work/state"
chmod 700 "$work/runtime"
export XDG_RUNTIME_DIR="$work/runtime" XDG_DATA_HOME="$work/data" XDG_CACHE_HOME="$work/cache" XDG_CONFIG_HOME="$work/config" XDG_STATE_HOME="$work/state"
export QT_QPA_PLATFORM=offscreen LIBGL_ALWAYS_SOFTWARE=1 GSETTINGS_BACKEND=memory QT_STYLE_OVERRIDE=Fusion APP_ID=com.fairphone6.Share
export FP6_SHARE_BIN_DIR=${FP6_SHARE_BIN_DIR:-$PWD/target/debug}
# No Quick Share/KDE/WebShare services or real devices exist on this bus.
xvfb-run -a dbus-run-session --config-file tests/session-bus.conf -- bash -eu -c '
  "$FP6_SHARE_BIN_DIR/fp6-share" daemon >"$XDG_STATE_HOME/daemon.log" 2>&1 & daemon=$!
  trap "kill $daemon 2>/dev/null || true" EXIT
  for n in $(seq 1 60); do
    if busctl --user --no-pager list | rg -q com.fairphone6.Share1; then break; fi
    sleep 0.1
  done
  "$FP6_SHARE_BIN_DIR/fp6-share-ui" --text "Synthetic live UI check" >"$XDG_STATE_HOME/ui.log" 2>&1 & ui=$!
  trap "kill $ui $daemon 2>/dev/null || true" EXIT
  sleep 4
  kill -0 "$ui"
  busctl --user call com.fairphone6.Share1 /com/fairphone6/Share1 com.fairphone6.Share1 Snapshot | rg -q "Synthetic live UI check"
  if rg "failed to load|TypeError|ReferenceError|SyntaxError|Unexpected token|is not a type|Cannot assign" "$XDG_STATE_HOME/ui.log"; then cat "$XDG_STATE_HOME/ui.log"; exit 1; fi
  kill "$ui"
  wait "$ui" || true
  sleep 3
  busctl --user call com.fairphone6.Share1 /com/fairphone6/Share1 com.fairphone6.Share1 Snapshot | rg -q "request.*null"
  echo "Live QML load, Content Hub component, D-Bus handoff and UI-close cleanup passed."
'
