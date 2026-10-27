#!/usr/bin/env bash
# "after" setup for round.sh on the app-ready build: zram on (system unit),
# the system fp6-app-ready service running, the measured apps kept ready +
# preloaded (KEEP / PRELOAD / PRELAUNCH override the lists). Undo: after-teardown.sh.
set -u
here=$(dirname "$(readlink -f "$0")")
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
export XDG_RUNTIME_DIR=/run/user/$(id -u)
grep -q "^/dev/zram0 " /proc/swaps || asroot swapon -p 100 /dev/zram0
systemctl --user start fp6-app-ready.service
"$here/ar-test.sh" gs set com.fairphone6.app-ready keep-ready "${KEEP:-['morph-browser', 'firefox', 'lomiri-messaging-app']}"
"$here/ar-test.sh" gs set com.fairphone6.app-ready preload "${PRELOAD:-['morph-browser', 'firefox', 'lomiri-messaging-app', 'lomiri-system-settings']}"
"$here/ar-test.sh" gs set com.fairphone6.app-ready prelaunch "${PRELAUNCH:-[]}"
sleep 3
systemctl --user is-active fp6-app-ready.service
