#!/usr/bin/env bash
# Undo after-setup.sh: daemon stopped, settings back to defaults, zram off.
here=$(dirname "$(readlink -f "$0")")
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
"$here/ar-test.sh" reset >/dev/null
"$here/ar-test.sh" stop
asroot "$here/zram-live.sh" off | head -2
