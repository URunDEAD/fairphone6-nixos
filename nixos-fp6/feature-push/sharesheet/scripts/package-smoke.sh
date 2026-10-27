#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work"/{runtime,state,data,config,cache}
chmod 700 "$work/runtime"
export XDG_RUNTIME_DIR="$work/runtime" XDG_STATE_HOME="$work/state" XDG_DATA_HOME="$work/data" XDG_CONFIG_HOME="$work/config" XDG_CACHE_HOME="$work/cache"
export QT_QPA_PLATFORM=offscreen LIBGL_ALWAYS_SOFTWARE=1 QT_STYLE_OVERRIDE=Fusion GSETTINGS_BACKEND=memory
package=$(readlink -f "${FP6_SHARE_PACKAGE:-$PWD/result}")
export FP6_SHARE_TEST_BIN="$package/bin"
sed "s|</busconfig>|<servicedir>$package/share/dbus-1/services</servicedir></busconfig>|" tests/session-bus.conf > "$work/activation.conf"
xvfb-run -a dbus-run-session --config-file "$work/activation.conf" -- bash -eu -c '
  "$FP6_SHARE_TEST_BIN/fp6-share" --text "Synthetic packaged CLI request"
  read -r type daemon < <(busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus GetConnectionUnixProcessID s com.fairphone6.Share1)
  test "$type" = u
  trap "kill $daemon 2>/dev/null || true" EXIT
  sleep 3
  busctl --user call com.fairphone6.Share1 /com/fairphone6/Share1 com.fairphone6.Share1 Snapshot | rg -q "Synthetic packaged CLI request"
  if busctl --user call com.fairphone6.Share1 /com/fairphone6/Share1 com.fairphone6.Share1 Attach 2>/dev/null; then
    echo "The child UI failed to attach" >&2

    exit 1
  fi
  kill -TERM "$daemon"

  echo "Packaged D-Bus activation launched the daemon and its QML UI successfully."
'
