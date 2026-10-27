#!/usr/bin/env bash
# Private D-Bus session with xdg-desktop-portal + xdg-desktop-portal-lomiri
# (D-Bus activated, as on the phone) and a synthetic $HOME; runs CMD in it.
#   test/portal-session.sh PKG XDP CMD...
# PKG = xdg-desktop-portal-lomiri store path, XDP = xdg-desktop-portal.
# Environment passed to the picker (inherited through D-Bus activation):
#   FP6_GRAB=/out.png:delay FP6_GRAB_SIZE=372x804 FP6_PREVIEW=step,step
set -eu
pkg=$1 xdp=$2
shift 2
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export HOME=${FAKE_HOME:-$tmp/home}
[ -d "$HOME/Downloads" ] || bash "$here/fake-home.sh" "$HOME"
export XDG_CONFIG_HOME=$HOME/.config XDG_DATA_HOME=$HOME/.local/share XDG_CACHE_HOME=$tmp/cache
export XDG_RUNTIME_DIR=$tmp/run
mkdir -p "$tmp/services" "$XDG_CONFIG_HOME/xdg-desktop-portal" "$XDG_RUNTIME_DIR"
chmod 700 "$XDG_RUNTIME_DIR"
cp "$xdp"/share/dbus-1/services/org.freedesktop.portal.Desktop.service "$tmp/services/"
cp "$pkg"/share/dbus-1/services/*.service "$tmp/services/"
cat > "$XDG_CONFIG_HOME/xdg-desktop-portal/portals.conf" <<EOF
[preferred]
default=none
org.freedesktop.impl.portal.FileChooser=lomiri
EOF
cat > "$tmp/session.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=$tmp</listen>
  <auth>EXTERNAL</auth>
  <servicedir>$tmp/services</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
export NIX_XDG_DESKTOP_PORTAL_DIR=$pkg/share/xdg-desktop-portal/portals
export XDG_CURRENT_DESKTOP=Lomiri
export XDG_DATA_DIRS=${SURU_ICONS:+$SURU_ICONS/share:}$pkg/share:${SHARED_MIME:-/run/current-system/sw}/share
# The laptop's Qt/theme environment must not leak into the Qt 5.15 picker.
unset QT_PLUGIN_PATH QML2_IMPORT_PATH QT_QPA_PLATFORMTHEME QT_STYLE_OVERRIDE WAYLAND_DISPLAY
export GRID_UNIT_PX=${GRID_UNIT_PX:-8} QT_SCALE_FACTOR=${QT_SCALE_FACTOR:-3} QT_QPA_PLATFORM=${QT_QPA_PLATFORM:-xcb}
mkdir -p "$XDG_CONFIG_HOME/lomiri-ui-toolkit"
printf '[General]\ntheme=%s\n' "${UITK_THEME:-Lomiri.Components.Themes.Ambiance}" > "$XDG_CONFIG_HOME/lomiri-ui-toolkit/theme.ini"
exec dbus-run-session --config-file="$tmp/session.conf" -- "$@"
