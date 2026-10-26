#!/usr/bin/env bash
# Restart the Lomiri session and log the phone user back in (phone side, as
# the phone user, caller holds the GUI lock and has checked that no camera
# capture runs). Login digits on stdin. Uses touch.py (ESWIN evdev) for the
# greeter swipe and tools/phone-type.py (short-lived uinput keyboard).
set -u
here=$(dirname "$(readlink -f "$0")")
read -r pin
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
export XDG_RUNTIME_DIR=/run/user/$(id -u)
screen_on() {
  gdbus call --system -d com.canonical.Unity.Screen -o /com/canonical/Unity/Screen \
    -m com.canonical.Unity.Screen.setScreenPowerMode on 1 >/dev/null 2>&1 || true
}
state() { systemctl --user show -p ActiveState --value lomiri.service; }
mainpid() { systemctl --user show -p MainPID --value lomiri.service; }
old=$(mainpid)
# A new Lomiri: active with a MainPID other than the one before the restart.
fresh() { [ "$(state)" = active ] && [ "$(mainpid)" != "$old" ] && [ "$(mainpid)" != 0 ]; }
echo "[$(date +%T)] restart display-manager (lomiri.service MainPID $old)"
asroot systemctl restart display-manager
for i in $(seq 1 15); do [ "$(state)" = inactive ] && break; sleep 1; done
# The running build's user Lomiri can outlive the display-manager restart
# (lomiri-session README, problem 2): stop it so the login starts a new one.
if [ "$(state)" != inactive ]; then
  echo "[$(date +%T)] lomiri.service still $(state): stopping it"
  timeout 20 systemctl --user stop lomiri.service || systemctl --user kill -s KILL lomiri.service
fi
# greeter up
for i in $(seq 1 30); do pgrep -u lightdm -x .lomiri-wrapped >/dev/null && break; sleep 1; done
sleep 8
for try in 1 2 3; do
  screen_on; sleep 1
  python3 "$here/touch.py" swipe 558 2100 558 700 350 >/dev/null
  sleep 2
  asroot python3 "$here/phone-type.py" "$pin" >/dev/null
  for i in $(seq 1 30); do fresh && break; sleep 1; done
  if fresh; then echo "[$(date +%T)] logged in (try $try), lomiri.service MainPID $(mainpid)"; exit 0; fi
done
echo "[$(date +%T)] login FAILED: lomiri.service $(state)"
exit 1
