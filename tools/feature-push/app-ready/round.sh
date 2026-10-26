#!/usr/bin/env bash
# One GUI-lock round (phone side): round.sh NAME SETUP 'APP MODES' ...
#   SETUP: none | relogin (install the test Lomiri drop-in, restart the session,
#          log in with the digits on stdin; at the end the drop-in is removed and
#          the session restarted on the system Lomiri again) | restore
#   env SETUP=script / TEARDOWN=script run after the login / before the restore.
# Holds /run/user/10000/fp6-gui.lock (waits up to 20 min), refuses while a
# camera capture runs, stops every app it measured, budget 9 min.
set -u
name=$1 setup=$2; shift 2
here=$(dirname "$(readlink -f "$0")")
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
# Session restarts only while no camera capture runs; plain app launches only
# need the GUI lock.
camera_busy() {
  case $setup in relogin|restore) pgrep -f "[l]omiri-camera-app|[g]st-launch|[l]ibcamera" >/dev/null ;; *) false ;; esac
}
# Take the lock only while no camera capture (or camera test round) runs;
# give it back and retry while one does (up to 40 min).
exec 9>"$XDG_RUNTIME_DIR/fp6-gui.lock"
waited=0
while :; do
  flock -w 1200 9 || { echo "no GUI lock"; exit 4; }
  camera_busy || break
  flock -u 9
  [ $waited -gt 2400 ] && { echo "camera busy"; exit 3; }
  sleep 30; waited=$((waited + 30))
done
echo "[$(date +%T)] lock held"
start=$(date +%s)
dropin=$XDG_RUNTIME_DIR/systemd/user/lomiri.service.d/zz-fp6-app-ready-test.conf
relogin() {  # relogin patched|system
  if [ "$1" = patched ]; then
    mkdir -p "$(dirname "$dropin")"
    printf '[Service]\nExecStart=\nExecStart=%s/lomiri-session\n' "$here" > "$dropin"
  else
    rm -f "$dropin"; rmdir "$(dirname "$dropin")" 2>/dev/null
  fi
  systemctl --user daemon-reload
  echo "$pin" | "$here/relogin.sh"
}
restored=yes
finish() {
  [ -n "${TEARDOWN:-}" ] && "$TEARDOWN"
  if [ "$restored" = no ]; then
    # Back to the system Lomiri, logged in, drop-in gone, before the lock is released.
    relogin system || relogin system
    restored=yes
  fi
  rm -f "$dropin"; rmdir "$(dirname "$dropin")" 2>/dev/null; systemctl --user daemon-reload
}
trap finish EXIT
case $setup in
  none) ;;
  relogin|restore)
    read -r pin
    if [ "$setup" = relogin ]; then
      restored=no
      relogin patched || exit 5
      sleep 12
    else
      relogin system || exit 5
    fi ;;
esac
[ -n "${SETUP:-}" ] && { "$SETUP" || { echo "setup $SETUP failed"; exit 6; }; }
LAL=${LAL:-$(dirname "$(ls /nix/store/*-lomiri-app-launch-*/bin/lomiri-app-launch | head -1)")}
apps=()
for spec in "$@"; do
  [ $(( $(date +%s) - start )) -gt ${BUDGET:-380} ] && { echo "time budget reached"; break; }
  set -- $spec; app=$1; shift
  apps+=("$app")
  timeout 150 "$here/measure.sh" "$here/out/$name" "$app" "$@"
done
for app in "${apps[@]}" ${OTHER:-lomiri-clock-app}; do
  for u in $(systemctl --user list-units --no-legend --plain "lomiri-app-launch--application-*--$app--*" | awk '{print $1}'); do
    timeout 10 systemctl --user stop "$u" || systemctl --user kill -s KILL "$u"
  done
done
sleep 2
echo "[$(date +%T)] round $name done in $(( $(date +%s) - start )) s"
