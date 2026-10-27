#!/usr/bin/env bash
# FP6 quick settings: the "Dark theme" tile. Keeps the dconf key
# /com/fairphone6/quick-settings/dark-theme and the app theme in sync, both ways:
#  * key changed (tile)        -> UITK theme.ini (Ambiance <-> SuruDark), GTK
#    colour scheme/theme, gtk-3.0 settings.ini, and the shell's light-mode when
#    display-comfort's "also switch the shell" is on (same files and keys as
#    display-comfort's dark-theme schedule and Settings > Background);
#  * theme.ini changed (Settings, display-comfort schedule) -> key.
# Only dconf and files; no polling.
set -u
key=/com/fairphone6/quick-settings/dark-theme
cfg=${XDG_CONFIG_HOME:-$HOME/.config}
ini=$cfg/lomiri-ui-toolkit/theme.ini
gtkini=$cfg/gtk-3.0/settings.ini
DARK=Lomiri.Components.Themes.SuruDark
LIGHT=Lomiri.Components.Themes.Ambiance

file_dark() { grep -q "^theme=$DARK\$" "$ini" 2>/dev/null && echo true || echo false; }
key_dark() { [ "$(dconf read "$key")" = true ] && echo true || echo false; }

apply() { # $1 = true|false
  local dark=$1 theme=$LIGHT tmp
  [ "$dark" = true ] && theme=$DARK
  mkdir -p "${ini%/*}" "${gtkini%/*}"
  tmp=$(mktemp "$ini.XXXXXX") && printf '[General]\ntheme=%s\n' "$theme" > "$tmp" && mv -f "$tmp" "$ini"
  if [ "$dark" = true ]; then
    dconf write /org/gnome/desktop/interface/color-scheme "'prefer-dark'"
    dconf write /org/gnome/desktop/interface/gtk-theme "'Adwaita-dark'"
  else
    dconf write /org/gnome/desktop/interface/color-scheme "'default'"
    dconf write /org/gnome/desktop/interface/gtk-theme "'Adwaita'"
  fi
  # gtk-3.0/settings.ini: [Settings] gtk-application-prefer-dark-theme
  [ -f "$gtkini" ] || printf '[Settings]\n' > "$gtkini"
  grep -q '^\[Settings\]' "$gtkini" || printf '[Settings]\n' >> "$gtkini"
  tmp=$(mktemp "$gtkini.XXXXXX")
  awk -v dark="$dark" '
    /^gtk-application-prefer-dark-theme=/ { next }
    { print }
    /^\[Settings\]$/ && dark == "true" { print "gtk-application-prefer-dark-theme=true" }
  ' "$gtkini" > "$tmp" && mv -f "$tmp" "$gtkini"
  if [ "$(dconf read /com/fairphone6/display-comfort/dark-theme-shell)" = true ]; then
    if [ "$dark" = true ]; then
      dconf write /com/lomiri/shell/light-mode false
    else
      dconf write /com/lomiri/shell/light-mode true
    fi
  fi
  echo "fp6-quick-settings-theme: dark theme $dark"
}

sync_key() { # theme.ini -> key
  local f; f=$(file_dark)
  [ "$(key_dark)" = "$f" ] || dconf write "$key" "$f"
}
sync_file() { # key -> theme.ini
  local k; k=$(key_dark)
  [ "$(file_dark)" = "$k" ] || apply "$k"
}

mkdir -p "${ini%/*}"
sync_key
dconf watch "$key" | while read -r line; do
  case $line in "$key"*|"") ;; *) continue ;; esac
  sync_file
done &
inotifywait -q -m -e close_write,moved_to,create --format %f "${ini%/*}" | while read -r f; do
  [ "$f" = theme.ini ] && sync_key
done &
wait
