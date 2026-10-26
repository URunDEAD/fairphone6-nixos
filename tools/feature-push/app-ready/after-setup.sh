#!/usr/bin/env bash
# "after" setup for round.sh: zram on, fp6-app-ready running, the measured
# apps kept ready + preloaded. Undo: after-teardown.sh.
set -u
here=$(dirname "$(readlink -f "$0")")
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
asroot "$here/zram-live.sh" on | head -3
bin=$(cat "$here/bin")
"$here/ar-test.sh" stop
"$here/ar-test.sh" gs set com.fairphone6.app-ready keep-ready "${KEEP:-['morph-browser', 'firefox', 'lomiri-messaging-app']}"
"$here/ar-test.sh" gs set com.fairphone6.app-ready preload "${PRELOAD:-['morph-browser', 'firefox', 'lomiri-messaging-app', 'lomiri-system-settings']}"
"$here/ar-test.sh" start "$(dirname "$bin")/fp6-app-ready"
sleep 3
systemctl --user is-active fp6-app-ready-test.service
