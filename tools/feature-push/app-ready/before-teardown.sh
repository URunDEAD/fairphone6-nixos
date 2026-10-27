#!/usr/bin/env bash
# Undo before-setup.sh: zram swap back (system unit).
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
grep -q "^/dev/zram0 " /proc/swaps || asroot swapon -p 100 /dev/zram0
swapon --show
