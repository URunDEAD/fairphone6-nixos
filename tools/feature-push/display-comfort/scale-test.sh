#!/bin/sh
# Display/text size test on the installed build (on the phone as `user`,
# FP6_PW set): set the size in GSettings (the daemon writes session.env), log
# in again through ONE display-manager restart, check the shell and app
# environment + screenshots, then restore the settings and end the test session
# (logind) so the next login is back at the defaults. Leaves the user logged in.
#   scale-test.sh GU PCT
set -u
GU=$1 PCT=$2
L="sh /tmp/fp6-dc-in/live-test.sh"
T=/tmp/fp6-dc
P=$T/lock.pid
S() { printf '%s\n' "$FP6_PW" | sudo -S -p '' "$@"; }
login_now() {
  for i in $(seq 1 30); do pgrep -u lightdm -x .lomiri-wrapped >/dev/null && break; sleep 1; done
  sleep 8
  for try in 1 2 3; do
    S gdbus call --system -d com.canonical.Unity.Screen -o /com/canonical/Unity/Screen \
      -m com.canonical.Unity.Screen.setScreenPowerMode on 1 >/dev/null 2>&1; sleep 1
    S python3 /tmp/fp6-dc-in/touch.py swipe 1000 1800 150 1800 300 >/dev/null; sleep 2
    S python3 /tmp/phone-type.py "$FP6_PW" >/dev/null
    for i in $(seq 1 30); do [ "$(systemctl --user show -p ActiveState --value lomiri.service)" = active ] && break; sleep 1; done
    [ "$(systemctl --user show -p ActiveState --value lomiri.service)" = active ] && break
  done
  sleep 8
}
report() {
  pid=$(systemctl --user show -p MainPID --value lomiri.service)
  sh_pid=$(pgrep -f "lomiri --mode=full" | head -1)
  echo "-- shell env ($1):"; tr '\0' '\n' < /proc/$sh_pid/environ | grep -E '^(GRID_UNIT_PX|QT_WAYLAND_FORCE_DPI|UITK_FONT_SCALE)='
  echo "-- app env ($1):"; systemctl --user show-environment | grep -E '^(GRID_UNIT_PX|QT_WAYLAND_FORCE_DPI|UITK_FONT_SCALE)='
  cat ~/.config/fp6-display-comfort/session.env 2>/dev/null | grep FP6_
}

$L attach /nix/store/m8g0l4q8vkmk4cw2gim3grbbpczlq7zs-fp6-displaycomfort-qml-0.1.0 \
          /nix/store/wpdy6sz22l2c3xdw97j5wxlgg0447g8y-fp6-display-comfort-0.1.0 || exit 1
report before
$L gs interface-scale $GU; $L gs font-scale $PCT; sleep 2
grep FP6_ ~/.config/fp6-display-comfort/session.env
$L keepon stop
echo "DM restart at $(date +%T)"
S systemctl restart display-manager
sleep 3
systemctl is-active -q display-manager || { S systemctl reset-failed display-manager; S systemctl start display-manager; }
for i in $(seq 1 15); do [ "$(systemctl --user show -p ActiveState --value lomiri.service)" = inactive ] && break; sleep 1; done
[ "$(systemctl --user show -p ActiveState --value lomiri.service)" = inactive ] || timeout 20 systemctl --user stop lomiri.service
login_now
report "test $GU/$PCT"
$L keepon start >/dev/null
$L shot scale-home
$L settings
$L keepon stop
# Restore the settings; the daemon writes the default session.env again.
$L gs interface-scale 24; $L gs font-scale 100; sleep 2
report "settings restored, still in the test session"
sid=$(loginctl list-sessions --no-legend | awk '$3 == "user" && $4 == "seat0" {print $1}')
echo "ending test session $sid at $(date +%T)"
S loginctl terminate-session "$sid"
for i in $(seq 1 20); do [ "$(systemctl --user show -p ActiveState --value lomiri.service)" = inactive ] && break; sleep 1; done
login_now
report restored
$L keepon start >/dev/null
$L shot scale-restored
$L teardown | tail -1
loginctl list-sessions --no-legend | grep seat0
echo FINISHED
