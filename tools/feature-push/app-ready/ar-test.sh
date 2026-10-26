#!/usr/bin/env bash
# app-ready live test helpers (phone side, as the phone user).
#   ar-test.sh start BIN      fp6-app-ready BIN as a transient user unit
#   ar-test.sh stop           stop it
#   ar-test.sh gs ARGS...     gsettings on com.fairphone6.app-ready (real dconf)
#   ar-test.sh reset          all com.fairphone6.app-ready keys back to defaults
#   ar-test.sh status         status JSON
set -u
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
state=$HOME/fp6-app-ready-test/bin
case $1 in
  start)
    echo "$2" > "$state"
    systemd-run --user --unit=fp6-app-ready-test -p KillMode=process -p Nice=5 "$2" run ;;
  stop) systemctl --user stop fp6-app-ready-test.service 2>/dev/null; systemctl --user reset-failed fp6-app-ready-test.service 2>/dev/null; true ;;
  gs|reset)
    bin=$(cat "$state"); wrapper=$(dirname "$bin")/fp6-app-ready
    GSETTINGS_SCHEMA_DIR=$(sed -n "s/^export GSETTINGS_SCHEMA_DIR='\(.*\)'/\1/p" "$wrapper")
    GIO_EXTRA_MODULES=$(grep -o "/nix/store/[^']*dconf[^']*/lib/gio/modules" "$wrapper" | head -1)
    export GSETTINGS_SCHEMA_DIR GIO_EXTRA_MODULES
    G=$(ls /nix/store/*-glib-*-bin/bin/gsettings | head -1)
    if [ "$1" = reset ]; then "$G" reset-recursively com.fairphone6.app-ready; "$G" list-recursively com.fairphone6.app-ready
    else shift; "$G" "$@"; fi ;;
  status) cat "$XDG_RUNTIME_DIR/fp6-app-ready/status.json" ;;
esac
