#!/bin/sh
set -eu
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/fp6-dictation"
mkdir -p "$config_dir"
exec qmlscene @qml@/Settings.qml "--config=$config_dir/settings.ini" "$@"
