#!/bin/sh
# Phase 2 GUI test session in the real Lomiri session. Holds
# /run/user/10000/fp6-gui.lock (at most 10 min) while active.
#   gui-session.sh start <lomiri-camera-app store path>
#   gui-session.sh stop
# stop: closes the app, restores the launcher wrapper and the app settings
# file, deletes the photos/videos the session created and releases the lock.
LAL=/nix/store/jbw28xzhdq37060ncfpx9qqn0q7qp7sk-lomiri-app-launch-0.2.0/bin
W=$HOME/.local/bin/fp6-lomiri-camera-app
B=/tmp/fp6-phase2-wrapper.bak
C=$HOME/.config/camera.ubports/camera.ubports.conf
M=/tmp/fp6-phase2-session.marker
P=/tmp/fp6-phase2-held.pid
busy() { pgrep -x "gst-launch-1.0|cam|qcam|fp6-capture|fp6-long-exposu|lomiri-camera-a|.lomiri-camera-|fp6camera-selft" >/dev/null; }
case $1 in
start)
  busy && { echo "camera busy"; exit 3; }
  rm -f $P
  setsid -f flock -w 900 /run/user/10000/fp6-gui.lock sh -c "echo \$\$ > $P; exec sleep 600" </dev/null >/dev/null 2>&1
  for i in $(seq 1 900); do [ -s $P ] && break; sleep 1; done
  [ -s $P ] || { echo "no GUI lock"; exit 4; }
  touch $M
  [ -f $B ] || cp -p $W $B
  [ -f $C.phase2bak ] || cp -p $C $C.phase2bak
  cat > $W <<W2
#!/bin/sh
# TEMPORARY Phase 2 GUI test (restored by gui-session.sh stop)
export FP6_CAMERA_CONFIG=\$HOME/.config/fp6-camera/camera.conf
exec $2/bin/lomiri-camera-app "\$@" 2>>/tmp/fp6-phase2-app.log
W2
  : > /tmp/fp6-phase2-app.log
  setsid -f $LAL/lomiri-app-launch lomiri-camera-app </dev/null >/dev/null 2>&1
  sleep 6; pgrep -af "[/]bin/lomiri-camera-app" | cut -c1-150
  ;;
stop)
  $LAL/lomiri-app-stop lomiri-camera-app 2>/dev/null || pkill -f "[/]bin/lomiri-camera-app"
  for i in $(seq 1 10); do pgrep -f "[/]bin/lomiri-camera-app" >/dev/null || break; sleep 1; done
  pkill -9 -f "[/]bin/lomiri-camera-app"; sleep 1
  [ -f $B ] && cp -p $B $W && rm -f $B
  [ -f $C.phase2bak ] && cp -p $C.phase2bak $C && rm -f $C.phase2bak
  if [ -f $M ]; then
    find $HOME/Pictures/camera.ubports $HOME/Videos/camera.ubports -type f -newer $M -print -delete 2>/dev/null
    rmdir $HOME/Pictures/camera.ubports/RAW 2>/dev/null
    rm -f $M
  fi
  [ -s $P ] && kill $(cat $P)
  rm -f $P
  tail -1 $W
  ;;
esac
