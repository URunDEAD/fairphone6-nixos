#!/bin/sh
# Display/text size live test (on the phone as `user`, FP6_PW set). One
# display-manager restart; the original session is restored by ending the test
# session and logging in again (no second DM restart).
#   scale-test.sh run LOMIRI_SESSION LOMIRI GU PCT
set -eu
T=/tmp/fp6-dc
P=$T/lock.pid
DROPIN=/run/user/10000/systemd/user/lomiri.service.d/zz-fp6-display-comfort-test.conf
ENVF=$HOME/.config/fp6-display-comfort/session.env
mkdir -p $T
S() { printf '%s\n' "$FP6_PW" | sudo -S -p '' "$@"; }
login() {
  for i in $(seq 1 30); do loginctl list-sessions --no-legend | grep -q "lightdm seat0 .* greeter" && break; sleep 1; done
  sleep 6
  # Horizontal drag: an upward swipe does not dismiss the cover page.
  S python3 /tmp/fp6-dc-in/touch.py swipe 1000 1800 150 1800 300 wait 1200
  S python3 /tmp/fp6-dc-in/touch.py tap 556 960 wait 500
  S python3 /tmp/phone-type.py "$FP6_PW"
  for i in $(seq 1 40); do pgrep -f "lomiri --mode=full" >/dev/null && break; sleep 1; done
  sleep 8
}
user_session() { loginctl list-sessions --no-legend | awk '$3 == "user" && $4 == "seat0" {print $1}'; }

case $1 in
run)
  LS=$2 LO=$3 GU=$4 PCT=$5
  setsid -f flock -w 900 /run/user/10000/fp6-gui.lock sh -c "echo \$\$ > $P; exec sleep 600" </dev/null >/dev/null 2>&1
  for i in $(seq 1 900); do [ -s $P ] && break; sleep 1; done
  [ -s $P ] || { echo "no GUI lock"; exit 4; }
  echo "lock held at $(date +%T)"
  [ -z "$(ls /run/user/10000/systemd/user/lomiri.service.d/ 2>/dev/null)" ] \
    || { echo "another lomiri drop-in is present; aborting"; kill "$(cat $P)"; rm -f $P; exit 5; }
  [ ! -e "$ENVF" ] || { echo "session.env exists already; aborting"; kill "$(cat $P)"; rm -f $P; exit 5; }
  mkdir -p "$(dirname $ENVF)" "$(dirname $DROPIN)" $T/cache2
  printf 'FP6_GRID_UNIT_PX=%s\nFP6_FONT_SCALE_PERCENT=%s\n' $GU $PCT > $ENVF
  cat > $DROPIN <<EOF
[Service]
ExecStart=
ExecStart=$LS/bin/lomiri-session
Environment=LOMIRI_BINARY=$LO/bin/lomiri
Environment=XDG_CACHE_HOME=$T/cache2
EOF
  systemctl --user daemon-reload
  echo "DM restart at $(date +%T)"
  S systemctl restart display-manager
  sleep 3
  if ! systemctl is-active -q display-manager; then
    S systemctl reset-failed display-manager; S systemctl start display-manager; sleep 3
  fi
  login
  pid=$(pgrep -f "lomiri --mode=full" | head -1)
  echo "shell: $(readlink /proc/$pid/exe)"
  tr '\0' '\n' < /proc/$pid/environ | grep -E '^(GRID_UNIT_PX|QT_WAYLAND_FORCE_DPI|UITK_FONT_SCALE)='
  systemctl --user show-environment | grep -E '^(GRID_UNIT_PX|QT_WAYLAND_FORCE_DPI|UITK_FONT_SCALE)='
  S python3 /tmp/drm-screenshot.py > $T/scale-home.png
  # Open System Settings (system build: Qt5 UITK app at its logical 8 px/GU).
  ( set -a; . /dev/stdin <<EOF
$(systemctl --user show-environment)
EOF
    set +a; setsid -f lomiri-app-launch lomiri-system-settings </dev/null >/dev/null 2>&1 ) || true
  sleep 8
  S python3 /tmp/drm-screenshot.py > $T/scale-settings.png
  lomiri-app-stop lomiri-system-settings 2>/dev/null || pkill -f bin/lomiri-system-settings || true
  sleep 2
  # Restore: no drop-in, no session.env, then end the test session and log in again.
  rm -f $DROPIN $ENVF; rmdir "$(dirname $ENVF)" 2>/dev/null || true
  systemctl --user daemon-reload
  sid=$(user_session)
  echo "ending test session $sid at $(date +%T)"
  S loginctl terminate-session "$sid"
  login
  systemctl --user unset-environment UITK_FONT_SCALE
  pid=$(pgrep -f "lomiri --mode=full" | head -1)
  echo "restored shell: $(readlink /proc/$pid/exe)"
  tr '\0' '\n' < /proc/$pid/environ | grep -E '^(GRID_UNIT_PX|QT_WAYLAND_FORCE_DPI|UITK_FONT_SCALE)='
  systemctl --user show-environment | grep -E '^(GRID_UNIT_PX|QT_WAYLAND_FORCE_DPI|UITK_FONT_SCALE)='
  S python3 /tmp/drm-screenshot.py > $T/scale-restored.png
  ls /run/user/10000/systemd/user/lomiri.service.d/ 2>/dev/null || true
  kill "$(cat $P)" 2>/dev/null || true; rm -f $P
  echo "done at $(date +%T)"
  ;;
esac
