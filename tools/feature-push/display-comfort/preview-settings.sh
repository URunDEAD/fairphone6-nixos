#!/usr/bin/env bash
# Laptop (x86) preview of System Settings > Display Comfort under Xvfb with the
# phone's scaling (GRID_UNIT_PX=8, QT_SCALE_FACTOR=3, 372x804 logical).
# GSettings use a private keyfile backend (no dconf, nothing of the laptop's
# own settings is read or written).
#   preview-settings.sh LSS_WRAPPED OUT.png PAGE [KEY=VALUE ...]
#   PAGE: the plugin name (display-comfort); sub pages through TAPS="x,y ...".
set -eu
lss=$1 out=$2 page=$3; shift 3
tmp=$(mktemp -d); xp=; trap '[ -n "$xp" ] && kill $xp 2>/dev/null; rm -rf "$tmp"' EXIT
export HOME=$tmp XDG_CONFIG_HOME=$tmp/config XDG_CACHE_HOME=$tmp/cache XDG_RUNTIME_DIR=$tmp
export GSETTINGS_BACKEND=keyfile
unset QT_PLUGIN_PATH QML2_IMPORT_PATH QT_QPA_PLATFORMTHEME QT_STYLE_OVERRIDE DBUS_SESSION_BUS_ADDRESS
mkdir -p $XDG_CONFIG_HOME/lomiri-ui-toolkit
printf '[General]\ntheme=%s\n' "${THEME:-Lomiri.Components.Themes.Ambiance}" > $XDG_CONFIG_HOME/lomiri-ui-toolkit/theme.ini
plug=$(nix-store -q --references "$(readlink -f $lss)" | grep display-comfort | head -1)
schemas=$(ls -d $plug/share/gsettings-schemas/*/glib-2.0/schemas)
for kv in "$@"; do GSETTINGS_SCHEMA_DIR=$schemas gsettings set com.fairphone6.display-comfort "${kv%%=*}" "${kv#*=}"; done
export GRID_UNIT_PX=8 QT_SCALE_FACTOR=3 QT_QPA_PLATFORM=xcb
Xvfb :77 -screen 0 1116x2412x24 >/dev/null 2>&1 & xp=$!
sleep 1
export DISPLAY=:77
# No D-Bus activatable services (portals would mount a FUSE dir in $tmp).
mkdir -p $tmp/empty
XDG_DATA_DIRS=$tmp/empty dbus-run-session -- "$lss/bin/lomiri-system-settings" "$page" >$tmp/log 2>&1 & ap=$!
sleep ${GRAB_DELAY:-8}
# TAPS="x,y x,y": physical-pixel taps (xdotool) before the grab, 2 s apart.
for t in ${TAPS:-}; do xdotool mousemove ${t%,*} ${t#*,} click 1; sleep 2; done
import -window root "$out"
kill $ap 2>/dev/null || true
grep -i "display-comfort\|qml\|error" $tmp/log | grep -v -i fontconfig | head -20 || true
