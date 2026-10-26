#!/usr/bin/env bash
# Camera fast-launch measurement (phone side, run as the phone user).
#   measure.sh OUT APP_BIN MODE... (MODE = cold | warm | resume)
# APP_BIN: a lomiri-camera-app bin directory's executable, e.g.
#   /run/current-system/sw/bin/lomiri-camera-app or /nix/store/...-lomiri-camera-app-4.1.1/bin/lomiri-camera-app
# Extra env for the app: FL_ENV="A=1 B=2".
#
# Holds /run/user/10000/fp6-gui.lock for the whole round (budget 9 min).
# For each mode: launch through lomiri-app-launch (what a launcher tap does),
# wait for the first frame + 6 s of AE, stop the app, keep the timing log.
#   cold   = sync + drop_caches before the launch (camera app not running)
#   warm   = launch right after a previous run (page cache warm)
#   resume = the app is already running in the background (Keep ready):
#            measured from the focus request to the next first frame
# The launcher wrapper ~/.local/bin/lomiri-camera-app exists only during the
# round (it routes the app's stderr into the journal with timestamps).
set -u
out=$1 app=$2; shift 2
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
LAL=$(dirname "$(readlink -f /etc/profiles/per-user/user/bin/lomiri 2>/dev/null)")
for d in /nix/store/*-lomiri-app-launch-*/bin; do [ -x "$d/lomiri-app-launch" ] && LAL=$d && break; done
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
busy() { pgrep -x "gst-launch-1.0|cam|qcam|fp6-capture|fp6-long-exposu|lomiri-camera-a|.lomiri-camera-|fp6camera-selft" >/dev/null; }
W=$HOME/.local/bin/lomiri-camera-app
M=/tmp/fp6-fl-session.marker
mkdir -p "$out"
busy && { echo "camera busy"; exit 3; }
[ -e "$W" ] && { echo "$W exists (another test?)"; exit 5; }

exec 9>/run/user/10000/fp6-gui.lock
flock -w 900 9 || { echo "no GUI lock"; exit 4; }
start=$(date +%s)
touch $M
stop_app() {  # lomiri-app-stop, then the unit, then the process
  "$LAL/lomiri-app-stop" lomiri-camera-app >/dev/null 2>&1
  for i in $(seq 1 10); do busy || return 0; sleep 0.5; done
  systemctl --user stop 'lomiri-app-launch--application-legacy--lomiri-camera-app--*.service' 2>/dev/null
  for i in $(seq 1 10); do busy || return 0; sleep 0.5; done
  pkill -x ".lomiri-camera-"; pkill -x "lomiri-camera-a"; sleep 1
}
cleanup() {
  stop_app
  pkill -9 -x ".lomiri-camera-" 2>/dev/null; pkill -9 -x "lomiri-camera-a" 2>/dev/null
  rm -f "$W"
  find $HOME/Pictures/camera.ubports $HOME/Videos/camera.ubports -type f -newer $M -delete 2>/dev/null
  rm -f $M
  [ "${locked:-}" = yes ] && asroot loginctl lock-session "$sess"
}
trap cleanup EXIT

sess=$(loginctl list-sessions --no-legend | awk '$2==10000 && $4=="seat0"{print $1; exit}')
if [ -z "$sess" ]; then
  # at the greeter: log in (PIN through a short-lived uinput keyboard, as
  # camera-phase2/gui-session.sh); the user stays logged in afterwards
  [ -n "${FP6_PW:-}" ] && [ -f /tmp/phone-type.py ] || { echo "no Lomiri user session on seat0"; exit 6; }
  gdbus call --system -d com.canonical.Unity.Screen -o /com/canonical/Unity/Screen \
    -m com.canonical.Unity.Screen.setScreenPowerMode on 1 >/dev/null 2>&1 || true
  sleep 1
  echo "$FP6_PW" | sudo -S -p '' python3 /tmp/phone-type.py "$FP6_PW" >/dev/null 2>&1
  for i in $(seq 1 30); do
    sess=$(loginctl list-sessions --no-legend | awk '$2==10000 && $4=="seat0"{print $1; exit}')
    [ -n "$sess" ] && systemctl --user is-active -q lomiri.service && break; sleep 1
  done
  [ -n "$sess" ] || { echo "login failed"; exit 6; }
  echo "logged in (session $sess)"; sleep 15   # let the session settle
fi
locked=$(loginctl show-session "$sess" -p LockedHint --value 2>/dev/null)
gdbus call --system -d com.canonical.Unity.Screen -o /com/canonical/Unity/Screen \
  -m com.canonical.Unity.Screen.setScreenPowerMode on 1 >/dev/null 2>&1 || true
sleep 1
[ "$locked" = yes ] && { asroot loginctl unlock-session "$sess"; sleep 2; }

mkdir -p "$(dirname "$W")"
cat > "$W" <<EOF
#!/bin/sh
# TEMPORARY camera fast-launch measurement (removed by measure.sh)
echo "FLMARK exec \$(date +%s.%N)" >&2
export GST_DEBUG=fp6camera:4 GST_DEBUG_NO_COLOR=1
export LIBCAMERA_LOG_LEVELS='*:WARN,Camera:INFO,IPASoftExposure:DEBUG'
export FP6_CAMERA_TRACE=1 ${FL_ENV:-}
exec systemd-cat -t fl-cam --level-prefix=false $app "\$@"
EOF
chmod +x "$W"

wait_for() {  # wait_for SINCE PATTERN TIMEOUT_S
  local t=0
  while [ $t -lt $(( $3 * 10 )) ]; do
    journalctl --user -t fl-cam --since "@$1" -o cat --no-pager 2>/dev/null | grep -q "$2" && return 0
    sleep 0.1; t=$((t + 1))
  done
  return 1
}

modes=("$@")
n=0
for mode in "$@"; do
  [ $(( $(date +%s) - start )) -gt 470 ] && { echo "time budget reached"; break; }
  n=$((n + 1))
  case $mode in
    cold)
      busy && { stop_app; sleep 1; }
      sync; asroot sh -c 'echo 3 > /proc/sys/vm/drop_caches'; sleep 1 ;;
    warm)
      busy && { stop_app; sleep 1; } ;;
    resume)
      busy || { echo "resume needs a running app"; continue; }
      # put the camera app in the background: focus the launcher/dash
      tb=$(date +%s)
      "$LAL/lomiri-app-launch" lomiri-system-settings >/dev/null 2>&1 &
      sleep 6
      # what the camera did in the background (does the sensor still stream?)
      journalctl --user -t fl-cam --since "@$tb" -o short-unix --no-pager > "$out/run$n-$mode.bg" ;;
  esac
  # FL_PRE: a command before each launch, e.g. the sensor back at its
  # driver defaults (= first use after a boot) for the AE seed test
  [ -n "${FL_PRE:-}" ] && sh -c "$FL_PRE"
  t0=$(date +%s.%N); ts=${t0%.*}
  "$LAL/lomiri-app-launch" lomiri-camera-app >/dev/null 2>&1 &
  echo "FLMARK launch $t0" > "$out/run$n-$mode.log"
  sleep ${FL_WAIT:-13}   # fixed wait: polling the journal would load the CPU during the launch
  wait_for "$ts" "first frame" 1 || echo "run $n ($mode): no first frame"
  journalctl --user -t fl-cam --since "@$ts" -o short-unix --no-pager >> "$out/run$n-$mode.log"
  # memory of the running app (Keep ready holds this while in the background)
  for p in $(pgrep -x ".lomiri-camera-"); do grep -E "^(Rss|Pss|Pss_Anon|Pss_File|Swap):" /proc/$p/smaps_rollup; done > "$out/run$n-$mode.mem" 2>/dev/null
  journalctl --user --since "@$ts" -o short-unix --no-pager | grep -E "lomiri-app-launch--application-legacy--lomiri-camera|ApplicationManager|lomiri-camera-app" >> "$out/run$n-$mode.sys" 2>/dev/null
  if [ "$mode" = resume ]; then "$LAL/lomiri-app-stop" lomiri-system-settings >/dev/null 2>&1; fi
  # keep the app for a following resume run; otherwise stop it
  case "${modes[$n]:-}" in resume) ;; *) stop_app ;; esac
  sleep 2
done
echo "done in $(( $(date +%s) - start )) s"
