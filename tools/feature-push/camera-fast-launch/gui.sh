#!/usr/bin/env bash
# Interactive GUI check (phone side) under the shared GUI lock (max 10 min).
#   gui.sh start APP_BIN [ENV...]  wait (foreground) until a holder process
#                                  really holds the lock (/proc/locks), then
#                                  launch the app via lomiri-app-launch
#   gui.sh touch ARGS...           tap/swipe (touch.py), only while we hold it
#   gui.sh shot > file.png         DRM screenshot, only while we hold it
#   gui.sh stop                    close the app, remove the wrapper, release
# Every GUI action checks the lock first and refuses without it.
set -u
export XDG_RUNTIME_DIR=/run/user/$(id -u)
LOCK=/run/user/10000/fp6-gui.lock
LAL=$(ls -d /nix/store/*-lomiri-app-launch-*/bin | head -1)
W=$HOME/.local/bin/lomiri-camera-app
P=/tmp/fl-gui-held.pid
M=/tmp/fl-gui.marker
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }

held() {  # our holder process is alive and owns the flock on the lock file
  local pid ino
  [ -s $P ] || return 1
  pid=$(cut -d" " -f1 $P); kill -0 "$pid" 2>/dev/null || return 1   # flock (owns the lock)
  ino=$(stat -c %i $LOCK)
  awk -v p="$pid" -v i="$ino" '$2=="FLOCK" && $5==p && $6 ~ (":" i "$") {f=1} END {exit !f}' /proc/locks
}

case $1 in
start)
  app=$2; shift 2
  pgrep -x ".lomiri-camera-|lomiri-camera-a|gst-launch-1.0" >/dev/null && { echo "camera busy"; exit 3; }
  [ -e $P ] && { echo "stale $P: run stop first"; exit 5; }
  setsid -f flock -w 900 $LOCK sh -c "echo \$PPID \$\$ > $P.new; mv $P.new $P; exec sleep 600" </dev/null >/dev/null 2>&1
  for i in $(seq 1 900); do held && break; sleep 1; done
  held || { echo "no GUI lock"; pkill -f "flock -w 900 $LOCK sh -c echo .* $P" ; exit 4; }
  echo "lock held by $(cut -d" " -f1 $P)"
  # at the greeter (another agent restarted the display manager): log in
  # (PIN via a short-lived uinput keyboard, FP6_PW); the user stays logged in
  if ! loginctl list-sessions --no-legend | awk '$2==10000 && $4=="seat0"' | grep -q .; then
    [ -n "${FP6_PW:-}" ] || { echo "no user session and no FP6_PW"; exit 7; }
    gdbus call --system -d com.canonical.Unity.Screen -o /com/canonical/Unity/Screen \
      -m com.canonical.Unity.Screen.setScreenPowerMode on 1 >/dev/null 2>&1 || true
    sleep 1
    echo "$FP6_PW" | sudo -S -p '' python3 /tmp/phone-type.py "$FP6_PW" >/dev/null 2>&1
    for i in $(seq 1 30); do systemctl --user is-active -q lomiri.service \
      && loginctl list-sessions --no-legend | awk '$2==10000 && $4=="seat0"' | grep -q . && break; sleep 1; done
    sleep 12; echo "logged in"
  fi
  touch $M
  printf '#!/bin/sh\n# TEMPORARY fast-launch GUI check (gui.sh stop removes it)\nexport %s\nexec systemd-cat -t fl-cam --level-prefix=false %s "$@"\n' "FP6_CAMERA_TRACE=1 $*" "$app" > $W
  chmod +x $W
  gdbus call --system -d com.canonical.Unity.Screen -o /com/canonical/Unity/Screen \
    -m com.canonical.Unity.Screen.setScreenPowerMode on 1 >/dev/null 2>&1 || true
  setsid -f $LAL/lomiri-app-launch lomiri-camera-app </dev/null >/dev/null 2>&1
  sleep 5; pgrep -x ".lomiri-camera-" >/dev/null && echo "app running" || { echo "app not running"; exit 6; }
  ;;
touch)
  shift; held || { echo "refused: GUI lock not held" >&2; exit 4; }
  asroot python3 /tmp/fl/touch.py "$@"
  ;;
shot)
  held || { echo "refused: GUI lock not held" >&2; exit 4; }
  asroot python3 /tmp/drmshot.py 2>/dev/null
  ;;
stop)
  $LAL/lomiri-app-stop lomiri-camera-app 2>/dev/null
  for i in $(seq 1 10); do pgrep -x ".lomiri-camera-" >/dev/null || break; sleep 0.5; done
  pkill -x ".lomiri-camera-"; sleep 1
  rm -f $W
  [ -f $M ] && find $HOME/Pictures/camera.ubports $HOME/Videos/camera.ubports -type f -newer $M -print -delete 2>/dev/null
  rm -f $M
  [ -s $P ] && kill $(cat $P) 2>/dev/null; rm -f $P   # flock and its sleep (both hold the fd)
  echo stopped
  ;;
esac
