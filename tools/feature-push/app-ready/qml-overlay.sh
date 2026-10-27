#!/usr/bin/env bash
# qml-overlay.sh on|off DIR: bind-mount the Settings page QML files from DIR
# over the installed plugin (test only, nothing persistent; off unmounts).
set -u
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
lss=$(dirname "$(readlink -f /run/current-system/sw/bin/lomiri-system-settings)")/..
plugin=$(readlink -f "$lss/share/lomiri-system-settings/qml-plugins/app-launch")
for f in "$2"/*.qml; do
  t=$(readlink -f "$plugin/$(basename "$f")")
  [ -e "$t" ] || continue
  if [ "$1" = on ]; then asroot mount --bind "$f" "$t"; else asroot umount "$t" 2>/dev/null; fi
done
findmnt -n -o TARGET | grep -c "qml-plugins/app-launch" || true
