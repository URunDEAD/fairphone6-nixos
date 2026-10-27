#!/usr/bin/env bash
# Offline UI preview on the laptop (x86): renders one app under Xvfb with
# the phone's Lomiri scaling (GRID_UNIT_PX=8, scale 3, 372x804 logical) and
# a fake IIO tree (batch 2: synthetic signals from fake-signals.py), and
# saves a PNG.
#   test/preview.sh /nix/store/...-fp6-sensor-apps-0.1.0 compass out.png [preview-page] [theme]
set -eu
pkg=$1 tool=$2 out=$3 page=${4:-} themeName=${5:-Lomiri.Components.Themes.Ambiance}
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export FP6_IIO_ROOT=$("$here/fake-iio.sh" "$tmp")
export XDG_CONFIG_HOME=$tmp/config XDG_STATE_HOME=$tmp/state HOME=$tmp
mkdir -p "$XDG_CONFIG_HOME/lomiri-ui-toolkit"
printf '[General]\ntheme=%s\n' "$themeName" > "$XDG_CONFIG_HOME/lomiri-ui-toolkit/theme.ini"
# no host Qt plugins/themes (the laptop's Qt may be another version)
unset QT_PLUGIN_PATH QML2_IMPORT_PATH QT_QPA_PLATFORMTHEME QT_STYLE_OVERRIDE
export XDG_DATA_DIRS=${SURU_ICONS:?set SURU_ICONS to a suru-icon-theme store path}/share
export GRID_UNIT_PX=8 QT_SCALE_FACTOR=3 QT_QPA_PLATFORM=xcb
export FP6_GRAB=$out:${GRAB_DELAY:-2500} FP6_GRAB_SIZE=372x804 FP6_PREVIEW=$page
export FP6_SKY_SOCKET=${FP6_SKY_SOCKET:-$tmp/none.sock}
if [ "$tool" = gps ] && [ -n "${FAKE_SKY:-}" ]; then
  export FP6_SKY_SOCKET=$tmp/sky.sock FP6_LOCATION_STATE=on
  python3 "$here/fake-sky.py" "$FP6_SKY_SOCKET" & sky=$!
  sleep 0.5
fi
# batch 2: synthetic signals only (no laptop microphone, speaker, camera or
# position is used)
export FP6_PW_PLAY=$here/fake-pw-play.sh FP6_NO_INDICATOR=1
mkdir -p "$tmp/led" && echo 255 > "$tmp/led/max_brightness" && echo 0 > "$tmp/led/brightness"
export FP6_TORCH_LED=$tmp/led
echo 50 > "$FP6_IIO_ROOT/iio:device2/sampling_frequency"
bin=$pkg/bin/fp6-sensor-tools
pids=
case $tool in
  steps) python3 "$here/fake-signals.py" accel-walk "$FP6_IIO_ROOT/iio:device2" & pids=$! ;;
  vibration) python3 "$here/fake-signals.py" accel-vibe "$FP6_IIO_ROOT/iio:device2" & pids=$! ;;
  metal) python3 "$here/fake-signals.py" magn-metal "$FP6_IIO_ROOT/iio:device3" & pids=$! ;;
  tuner) export FP6_PW_RECORD=$here/fake-pw-record.sh ;;
  speed)
    if [ -n "${FAKE_FIX:-}" ]; then
      export FP6_LOCATION_STATE=on FP6_NMEA_DEMO=$tmp/demo.nmea
      python3 "$here/fake-signals.py" nmea "$FP6_NMEA_DEMO"
    fi ;;
  magnifier) bin=$pkg/bin/fp6-magnifier ;;
esac
xvfb-run -a -s "-screen 0 1200x2600x24" "$bin" "$tool"
[ -n "${sky:-}" ] && kill "$sky" || true
[ -n "$pids" ] && kill $pids || true
