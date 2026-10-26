#!/usr/bin/env bash
# Offline UI preview on the laptop (x86): phone scaling (GRID_UNIT_PX=8,
# scale 3, 372x804 logical) under Xvfb, sample images in a temp HOME, and the
# real helper on a private session bus over a FAKE configfs tree (nothing on
# the laptop's USB is touched).
#   test/preview.sh PKG out.png [page] [theme] [state]
#     page:  "" | host | downloads | settings | create
#     state: idle | hosting | exclusive
set -eu
pkg=$1 out=$2 page=${3:-} themeName=${4:-Lomiri.Components.Themes.Ambiance} state=${5:-idle}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export HOME=$tmp/home XDG_CONFIG_HOME=$tmp/home/.config XDG_STATE_HOME=$tmp/home/.local/state
mkdir -p "$HOME/Images" "$XDG_CONFIG_HOME/lomiri-ui-toolkit"
printf '[General]\ntheme=%s\n' "$themeName" > "$XDG_CONFIG_HOME/lomiri-ui-toolkit/theme.ini"
# sample library
here=$(cd "$(dirname "$0")" && pwd)
python3 "$here/make-samples.py" "$HOME/Images"
# fake gadget
g=$tmp/cfs/g1
mkdir -p "$g/configs/c.1" "$g/functions/ncm.usb0" "$tmp/udc/a600000.usb"
ln -s "$g/functions/ncm.usb0" "$g/configs/c.1/rndis"
echo a600000.usb > "$g/UDC"
echo configured > "$tmp/udc/a600000.usb/state"
export FP6_USB_DRIVE_SESSION_BUS=1 FP6_USB_DRIVE_CATALOG=$here/../downloads.json
unset QT_PLUGIN_PATH QML2_IMPORT_PATH QT_QPA_PLATFORMTHEME QT_STYLE_OVERRIDE
export XDG_DATA_DIRS=${SURU_ICONS:?set SURU_ICONS}/share
export GRID_UNIT_PX=8 QT_SCALE_FACTOR=3 QT_QPA_PLATFORM=xcb
export FP6_GRAB=$out:${GRAB_DELAY:-3000} FP6_GRAB_SIZE=372x804 FP6_PREVIEW=$page
# Private bus without service activation (no portals from the laptop).
cat > "$tmp/bus.conf" <<CONF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:path=$tmp/bus</listen><auth>EXTERNAL</auth>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>
CONF
dbus-daemon --config-file="$tmp/bus.conf" --fork --print-pid > "$tmp/bus.pid"
export DBUS_SESSION_BUS_ADDRESS=unix:path=$tmp/bus
bash -c '
  '"$pkg"'/bin/fp6-usb-drived --session-bus --fake-configfs --no-polkit --uid '"$(id -u)"' \
     --gadget '"$g"' --udc-class '"$tmp"'/udc --lock '"$tmp"'/lock --state '"$tmp"'/run/state.json \
     --allow "$HOME/Images" >'"$tmp"'/helper.log 2>&1 &
  hp=$!
  for i in $(seq 50); do '"$pkg"'/bin/fp6-usb-drive-cli status >/dev/null 2>&1 && break; sleep 0.1; done
  case '"$state"' in
    hosting) '"$pkg"'/bin/fp6-usb-drive-cli host "$HOME/Images/nixos-minimal-26.05-x86_64-linux.iso" cdrom >/dev/null ;;
    exclusive) '"$pkg"'/bin/fp6-usb-drive-cli host "$HOME/Images/debian-13.7.0-amd64-netinst.iso" disk-ro exclusive >/dev/null ;;
  esac
  xvfb-run -a -s "-screen 0 1200x2600x24" '"$pkg"'/bin/fp6-usb-drive
  kill $hp
'
kill "$(cat "$tmp/bus.pid")" 2>/dev/null || true
