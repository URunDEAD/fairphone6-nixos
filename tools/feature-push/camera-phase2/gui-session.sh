#!/bin/sh
# Phase 2 GUI test session in the real Lomiri session. Holds
# /run/user/10000/fp6-gui.lock (at most 10 min) while active.
#   gui-session.sh start <lomiri-camera-app store path>   (FP6_PW in the
#        environment logs in at the greeter if no session is active)
#   gui-session.sh stop
# The test build is started directly with the session's environment; no
# launcher, wrapper or .desktop file is changed. stop: closes it, restores
# the app settings file, deletes the photos/videos the session created and
# releases the lock.
C=$HOME/.config/camera.ubports/camera.ubports.conf
M=/tmp/fp6-phase2-session.marker
P=/tmp/fp6-phase2-held.pid
A=/tmp/fp6-phase2-app.pid
busy() { pgrep -x "gst-launch-1.0|cam|qcam|fp6-capture|fp6-long-exposu|lomiri-camera-a|.lomiri-camera-|fp6camera-selft" >/dev/null; }
case $1 in
start)
  rm -f $P
  setsid -f flock -w 900 /run/user/10000/fp6-gui.lock sh -c "echo \$\$ > $P; exec sleep 600" </dev/null >/dev/null 2>&1
  for i in $(seq 1 900); do [ -s $P ] && break; sleep 1; done
  [ -s $P ] || { echo "no GUI lock"; exit 4; }
  # with the lock held, wait (up to 5 min) until nobody uses the camera
  for i in $(seq 1 60); do busy || break; sleep 5; done
  busy && { echo "camera busy"; kill $(cat $P); rm -f $P; exit 3; }
  if ! loginctl list-sessions --no-legend | grep -q "user *seat0 .* user "; then
    [ -n "$FP6_PW" ] || { echo "not logged in"; kill $(cat $P); exit 5; }
    echo "$FP6_PW" | sudo -S -p '' python3 /tmp/phone-type.py "$FP6_PW" >/dev/null 2>&1
  fi
  # the session's environment appears a few seconds after the login
  for i in $(seq 1 40); do
    systemctl --user show-environment 2>/dev/null | grep -q '^WAYLAND_DISPLAY=.' && break
    sleep 1
  done
  touch $M
  [ -f $C.phase2bak ] || cp -p $C $C.phase2bak
  : > /tmp/fp6-phase2-app.log
  # the environment lomiri-app-launch gives apps (systemd user manager)
  systemctl --user show-environment > /tmp/fp6-phase2-env
  grep -q '^WAYLAND_DISPLAY=.' /tmp/fp6-phase2-env || { echo "no wayland session"; kill $(cat $P); rm -f $P $M; exit 6; }
  ( cd $HOME; exec bash -c "set -a; . /tmp/fp6-phase2-env; set +a; export APP_ID=lomiri-camera-app;
    setsid -f sh -c 'echo \$\$ > $A; exec $2/bin/lomiri-camera-app >>/tmp/fp6-phase2-app.log 2>&1' </dev/null" )
  sleep 7; pgrep -af "[/]bin/lomiri-camera-app" | cut -c1-150
  ;;
stop)
  [ -s $A ] && kill $(cat $A) 2>/dev/null
  for i in $(seq 1 10); do pgrep -f "[/]bin/lomiri-camera-app" >/dev/null || break; sleep 1; done
  pkill -9 -f "[/]bin/lomiri-camera-app"; sleep 1
  rm -f $A /tmp/fp6-phase2-env
  [ -f $C.phase2bak ] && cp -p $C.phase2bak $C && rm -f $C.phase2bak
  if [ -f $M ]; then
    find $HOME/Pictures/camera.ubports $HOME/Videos/camera.ubports -type f -newer $M -print -delete 2>/dev/null
    rmdir $HOME/Pictures/camera.ubports/RAW 2>/dev/null
    rm -f $M
  fi
  [ -s $P ] && kill $(cat $P)
  rm -f $P
  echo stopped
  ;;
esac
