#!/usr/bin/env bash
# "before" conditions on the app-ready build: no zram swap, nothing kept
# ready (lists empty). Undo: before-teardown.sh.
here=$(dirname "$(readlink -f "$0")")
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
"$here/ar-test.sh" reset >/dev/null
asroot systemctl stop dev-zram0.swap
swapon --show
