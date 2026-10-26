#!/bin/sh
# privacy-lock live test in the real Lomiri session (runs ON the phone as
# `user`; FP6_PW in the environment for sudo and the greeter login).
#   live-test.sh setup PKG SCHEMA PATCH
#       take the GUI lock; copy the RUNNING Lomiri's Shell.qml and Greeter/,
#       apply PATCH (lomiri-privacy-lock.patch) and bind-mount them over the
#       running Lomiri's (never files of a newer build: they may import QML
#       modules the running build lacks); runtime lomiri.service
#       drop-in (schema + indicator dir in
#       XDG_DATA_DIRS, fresh QML cache); test dirs /run/fp6-privacy and
#       /var/lib/fp6-privacy; ONE display-manager restart; log in; start the
#       daemons as transient units (fp6-privacyd with --dry-run: SOS can never
#       call or text) and the root camera monitor.
#   live-test.sh gs KEY VALUE | get    test schema (com.fairphone6.privacy)
#   live-test.sh shot NAME             DRM screenshot -> /tmp/fp6-pl/NAME.png
#   live-test.sh tap X Y | hold X Y MS | swipe X1 Y1 X2 Y2 MS
#   live-test.sh screen on|off         Unity.Screen setScreenPowerMode (reason power key)
#   live-test.sh teardown              reset the keys, stop units, remove drop-in,
#       bind mounts and test dirs, release the lock (no second DM restart).
set -eu
T=/tmp/fp6-pl
P=$T/lock.pid
RUN=$(readlink -f "$(command -v lomiri)" | sed 's|/bin/lomiri$||')
DROPIN=/run/user/10000/systemd/user/lomiri.service.d/zz-fp6-privacy-lock-test.conf
SCHEMA_ID=com.fairphone6.privacy
mkdir -p $T
sudo_() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p '' "$@"; }
gsenv() { cat $T/schema-env 2>/dev/null; }
held() { [ -s $P ] && kill -0 "$(cat $P)" 2>/dev/null; }
touchpy=/tmp/fp6-pl-in/touch.py

case $1 in
setup)
  PKG=$2 SCHEMA=$3 PATCH=$4
  setsid -f flock -w 900 /run/user/10000/fp6-gui.lock sh -c "echo \$\$ > $P; exec sleep 600" </dev/null >/dev/null 2>&1
  for i in $(seq 1 900); do [ -s $P ] && break; sleep 1; done
  held || { echo "no GUI lock"; exit 4; }
  echo "lock held by $(cat $P) at $(date +%T)"
  [ -z "$(ls /run/user/10000/systemd/user/lomiri.service.d/ 2>/dev/null | grep -v zz-fp6-privacy-lock-test)" ] \
    || { echo "another lomiri drop-in is present; aborting"; kill "$(cat $P)"; rm -f $P; exit 5; }
  schemadir=$(ls -d $SCHEMA/share/gsettings-schemas/*)
  echo "XDG_DATA_DIRS=$schemadir:$T/share:$(systemctl --user show-environment | sed -n 's/^XDG_DATA_DIRS=//p')" > $T/schema-env
  # before state
  for k in activity-timeout dim-timeout; do echo "$k=$(gsettings get com.lomiri.touch.system $k)"; done > $T/touch.before
  mkdir -p $T/share/ayatana/indicators $T/cache
  cat > $T/share/ayatana/indicators/com.lomiri.indicator.privacy <<EOF
[Indicator Service]
Name=fp6-privacy-indicator
ObjectPath=/com/lomiri/indicator/privacy
Position=5

[phone]
ObjectPath=/com/lomiri/indicator/privacy/phone
Position=5

[desktop]
ObjectPath=/com/lomiri/indicator/privacy/desktop
EOF
  # shell side
  rm -rf $T/qml; mkdir -p $T/qml
  cp $RUN/share/lomiri/Shell.qml $T/qml/; cp -r $RUN/share/lomiri/Greeter $T/qml/; chmod -R u+w $T/qml
  sed 's|a/qml/|a/|; s|b/qml/|b/|' "$PATCH" | (cd $T/qml && patch -p1 --no-backup-if-mismatch) \
    || { echo "patch does not apply to the running Lomiri"; kill "$(cat $P)"; rm -f $P; exit 6; }
  sudo_ mount --bind $T/qml/Shell.qml $RUN/share/lomiri/Shell.qml
  sudo_ mount --bind $T/qml/Greeter $RUN/share/lomiri/Greeter
  # test dirs (as the module's tmpfiles rules)
  sudo_ install -d -m 2750 -o root -g users /run/fp6-privacy
  sudo_ install -d -m 2750 -o user -g lightdm /var/lib/fp6-privacy
  mkdir -p "$(dirname $DROPIN)"
  cat > $DROPIN <<EOF
[Service]
Environment="$(gsenv)"
Environment=XDG_CACHE_HOME=$T/cache
EOF
  systemctl --user daemon-reload
  echo "DM restart at $(date +%T)"
  sudo_ systemctl restart display-manager
  sleep 12
  if ! loginctl list-sessions --no-legend | grep -q "user *seat0"; then
    sudo_ python3 /tmp/fp6-pl-in/phone-type.py "$FP6_PW" >/dev/null 2>&1 || true
  fi
  for i in $(seq 1 40); do
    pgrep -f "lomiri --mode=full-shell" >/dev/null && systemctl --user is-active -q lomiri.service && break
    sleep 1
  done
  sleep 5
  sudo_ systemd-run --unit=fp6-pl-cam --collect -p Nice=10 $PKG/bin/fp6-privacy-system camera-monitor >/dev/null
  E="$(gsenv)"
  systemd-run --user --unit=fp6-pl-ind --collect -E "$E" -E GSETTINGS_SCHEMA_DIR=$schemadir/glib-2.0/schemas $PKG/bin/fp6-privacy-indicator >/dev/null
  systemd-run --user --unit=fp6-pl-d --collect -E "$E" -E GSETTINGS_SCHEMA_DIR=$schemadir/glib-2.0/schemas $PKG/bin/fp6-privacyd run --dry-run >/dev/null
  pgrep -af "lomiri --mode" | cut -c1-80; systemctl --user is-active fp6-pl-ind fp6-pl-d || true
  ;;
gs)
  env "$(gsenv)" gsettings set $SCHEMA_ID "$2" "$3"
  ;;
get)
  env "$(gsenv)" gsettings list-recursively $SCHEMA_ID
  ;;
shot)
  sudo_ python3 /tmp/fp6-pl-in/drm-screenshot.py > $T/$2.png
  ls -la $T/$2.png
  ;;
tap|hold|swipe)
  sudo_ python3 $touchpy "$@"
  ;;
screen)
  sudo_ gdbus call --system -d com.canonical.Unity.Screen -o /com/canonical/Unity/Screen -m com.canonical.Unity.Screen.setScreenPowerMode "$2" 2 >/dev/null
  ;;
teardown)
  systemctl --user stop fp6-pl-ind fp6-pl-d 2>/dev/null || true
  sudo_ systemctl stop fp6-pl-cam 2>/dev/null || true
  dconf reset -f /com/fairphone6/privacy/
  while IFS='=' read -r k v; do gsettings set com.lomiri.touch.system $k "$v"; done < $T/touch.before
  rm -f $DROPIN; rmdir "$(dirname $DROPIN)" 2>/dev/null || true; systemctl --user daemon-reload
  for m in $(findmnt -rn -o TARGET | grep -E "$RUN/share/lomiri/(Shell.qml|Greeter)$"); do sudo_ umount "$m" || true; done
  findmnt -rn -o TARGET | grep -cE "lomiri/(Shell.qml|Greeter)" || true
  sudo_ rm -rf /run/fp6-privacy /var/lib/fp6-privacy
  rm -rf /run/user/10000/fp6-privacy
  [ -s $P ] && kill "$(cat $P)" 2>/dev/null || true
  rm -f $P
  echo "teardown done at $(date +%T)"
  ;;
esac
