#!/usr/bin/env bash
# Laptop test of fp6-quick-settings-theme on a private session bus and HOME.
#   nix-shell -p dconf inotify-tools dbus --run 'test/theme-helper-test.sh HELPER'
set -u
helper=$1
export HOME=$(mktemp -d) XDG_CONFIG_HOME= XDG_RUNTIME_DIR=$(mktemp -d)
unset XDG_CONFIG_HOME
dcs=$(dirname "$(readlink -f "$(command -v dconf)")")/../share/dbus-1/services
export XDG_DATA_DIRS=$dcs/../..:${XDG_DATA_DIRS:-}
dbus-run-session -- bash -c '
  set -u
  setsid "'"$helper"'" & pid=$!
  sleep 1
  ini=$HOME/.config/lomiri-ui-toolkit/theme.ini
  dconf write /com/fairphone6/quick-settings/dark-theme true; sleep 1
  grep -q SuruDark "$ini" && echo "PASS tile on -> theme.ini SuruDark" || echo "FAIL tile on"
  [ "$(dconf read /org/gnome/desktop/interface/color-scheme)" = "'"'"'prefer-dark'"'"'" ] && echo "PASS gtk color-scheme" || echo "FAIL gtk color-scheme"
  grep -q "gtk-application-prefer-dark-theme=true" $HOME/.config/gtk-3.0/settings.ini && echo "PASS gtk settings.ini" || echo "FAIL settings.ini"
  printf "[General]\ntheme=Lomiri.Components.Themes.Ambiance\n" > "$ini"; sleep 1
  [ "$(dconf read /com/fairphone6/quick-settings/dark-theme)" = false ] && echo "PASS theme.ini light -> key false" || echo "FAIL file->key"
  dconf write /com/fairphone6/quick-settings/dark-theme false; sleep 1
  grep -q Ambiance "$ini" && echo "PASS stays light (no loop)" || echo "FAIL loop"
  dconf write /com/fairphone6/display-comfort/dark-theme-shell true
  dconf write /com/fairphone6/quick-settings/dark-theme true; sleep 1
  [ "$(dconf read /com/lomiri/shell/light-mode)" = false ] && echo "PASS shell light-mode follows (dark-theme-shell)" || echo "FAIL shell"
  kill -- -$pid 2>/dev/null; true
'
rm -rf "$HOME" "$XDG_RUNTIME_DIR"
