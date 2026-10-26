#!/bin/sh
# Display comfort live test in the real Lomiri session (runs ON the phone as
# `user`; FP6_PW in the environment for sudo and the greeter login).
#   live-test.sh setup PLUGIN DAEMON SHELLSCREEN.qml   take the GUI lock, bind-mount
#        the patched ShellScreen.qml over the running Lomiri's, runtime drop-in
#        with the plugin/schema paths, restart display-manager ONCE, log in,
#        start the daemon as a transient unit
#   live-test.sh gs KEY VALUE        gsettings set com.fairphone6.display-comfort (test schema)
#   live-test.sh shot NAME           DRM framebuffer screenshot -> /tmp/fp6-dc/NAME.png
#   live-test.sh load start|stop     full-screen animated qmlscene (constant redraw)
#   live-test.sh sample SECONDS      lomiri CPU, GPU devfreq residency, battery power
#   live-test.sh teardown            reset every key the test set, stop the daemon
#        and load, remove drop-in and bind mount, release the lock. No second
#        DM restart: the running shell keeps the (neutral, node-less) filter
#        item until the next login.
set -eu
T=/tmp/fp6-dc
P=$T/lock.pid
LOMIRI=$(readlink -f "$(command -v lomiri)" | sed 's|/bin/lomiri$||')
DROPIN=/run/user/10000/systemd/user/lomiri.service.d/zz-fp6-display-comfort-test.conf
SCHEMA_ID=com.fairphone6.display-comfort
mkdir -p $T
sudo_() { printf '%s\n' "$FP6_PW" | sudo -S -p '' "$@"; }
gsenv() { cat $T/schema-env 2>/dev/null; }
held() { [ -s $P ] && kill -0 "$(cat $P)" 2>/dev/null; }

case $1 in
attach|setup)
  PLUGIN=$2 DAEMON=$3 QML=${4:-}
  setsid -f flock -w 900 /run/user/10000/fp6-gui.lock sh -c "echo \$\$ > $P; exec sleep 600" </dev/null >/dev/null 2>&1
  for i in $(seq 1 900); do [ -s $P ] && break; sleep 1; done
  held || { echo "no GUI lock"; exit 4; }
  echo "lock held by $(cat $P) at $(date +%T)"
  [ "$1" = attach ] || [ -z "$(ls /run/user/10000/systemd/user/lomiri.service.d/ 2>/dev/null | grep -v zz-fp6-display-comfort)" ] \
    || { echo "another lomiri drop-in is present; aborting"; kill "$(cat $P)"; rm -f $P; exit 5; }
  schemadir=$(ls -d $DAEMON/share/gsettings-schemas/*)
  echo "XDG_DATA_DIRS=$schemadir:$(systemctl --user show-environment | sed -n 's/^XDG_DATA_DIRS=//p')" > $T/schema-env
  # Before state (for teardown and the report).
  cp -p ~/.config/lomiri-ui-toolkit/theme.ini $T/theme.ini.before
  cp -p ~/.config/gtk-3.0/settings.ini $T/gtk3.before 2>/dev/null || echo none > $T/gtk3.before
  for k in color-scheme gtk-theme text-scaling-factor; do
    echo "$k=$(dconf read /org/gnome/desktop/interface/$k)"; done > $T/dconf.before
  gsettings get com.lomiri.Shell light-mode > $T/lightmode.before
  if [ "$1" = attach ]; then
    # The running shell already has the filter (started by an earlier setup).
    tr '\0' '\n' < /proc/$(pgrep -f "lomiri --mode=full" | head -1)/environ | grep -q "$PLUGIN" \
      || { echo "running shell has no filter plugin"; kill "$(cat $P)"; rm -f $P; exit 7; }
    systemd-run --user --unit=fp6-dc-test --collect $DAEMON/bin/fp6-display-comfort run >/dev/null
    systemctl --user is-active fp6-dc-test
    exit 0
  fi
  # Shell side: patched ShellScreen.qml + plugin + schema, fresh QML cache.
  install -m644 "$QML" $T/ShellScreen.qml
  sudo_ mount --bind $T/ShellScreen.qml $LOMIRI/share/lomiri/ShellScreen.qml
  mkdir -p "$(dirname $DROPIN)" $T/cache
  cat > $DROPIN <<EOF
[Service]
Environment="QML2_IMPORT_PATH=$PLUGIN/lib/qt-5.15.19/qml"
Environment="$(gsenv)"
Environment=XDG_CACHE_HOME=$T/cache
EOF
  systemctl --user daemon-reload
  echo "DM restart at $(date +%T)"
  sudo_ systemctl restart display-manager
  sleep 12
  if ! loginctl list-sessions --no-legend | grep -q "user *seat0"; then
    sudo_ python3 /tmp/phone-type.py "$FP6_PW" >/dev/null 2>&1 || true
  fi
  for i in $(seq 1 40); do
    systemctl --user show-environment 2>/dev/null | grep -q '^WAYLAND_DISPLAY=.' && pgrep -x lomiri >/dev/null && break
    sleep 1
  done
  sleep 5
  systemd-run --user --unit=fp6-dc-test --collect $DAEMON/bin/fp6-display-comfort run >/dev/null
  pgrep -a -x lomiri; systemctl --user is-active fp6-dc-test
  ;;
gs)
  env "$(gsenv)" gsettings set $SCHEMA_ID "$2" "$3"
  ;;
get)
  env "$(gsenv)" gsettings list-recursively $SCHEMA_ID
  ;;
shot)
  sudo_ python3 /tmp/drm-screenshot.py > $T/$2.png
  ls -la $T/$2.png
  ;;
load)
  if [ "$2" = start ]; then
    QB=$(ldd /nix/store/2wwhdilq54vnf32a1815n19v902skmb6-qtdeclarative-5.15.19-dev/bin/qmlscene | sed -n 's|.*=> \(/nix/store/[^/]*-qtbase-5.15.19\)/lib/libQt5Core.*|\1|p')
    WL=
    for d in /nix/store/*-qtwayland-5.15.19-bin; do
      f=$d/lib/qt-5.15.19/plugins/platforms/libqwayland-egl.so
      [ -f $f ] && ldd $f | grep -q "$QB/lib/libQt5Gui" && WL=$d && break
    done
    [ -n "$WL" ] || { echo "no matching qtwayland"; exit 6; }
    cat > $T/load.qml <<'EOF'
import QtQuick 2.15
import QtQuick.Window 2.15
Window {
    id: w
    visible: true; visibility: Window.FullScreen; color: "#3060a0"
    property int frames: 0
    onFrameSwapped: frames++
    Timer { interval: 5000; repeat: true; running: true
        onTriggered: { console.log("fps " + (w.frames / 5).toFixed(1)); w.frames = 0 } }
    Rectangle { width: 200; height: 200; color: "#e0a030"; anchors.centerIn: parent
        RotationAnimation on rotation { from: 0; to: 360; duration: 2000; loops: Animation.Infinite } }
    Row { anchors.bottom: parent.bottom
        Repeater { model: ["#ffffff", "#ff0000", "#00ff00", "#0000ff", "#808080", "#000000"]
            Rectangle { width: 62; height: 120; color: modelData } } }
}
EOF
    ( set -a; . /dev/stdin <<EOF
$(systemctl --user show-environment)
EOF
      set +a; export QML2_IMPORT_PATH=$(ldd /nix/store/2wwhdilq54vnf32a1815n19v902skmb6-qtdeclarative-5.15.19-dev/bin/qmlscene | sed -n 's|.*=> \(/nix/store/[^/]*-qtdeclarative-5.15.19\)/lib/libQt5Quick.so.*|\1|p')/lib/qt-5.15.19/qml
      export QT_PLUGIN_PATH=$WL/lib/qt-5.15.19/plugins
      setsid -f sh -c "echo \$\$ > $T/load.pid; exec /nix/store/2wwhdilq54vnf32a1815n19v902skmb6-qtdeclarative-5.15.19-dev/bin/qmlscene $T/load.qml >$T/load.log 2>&1" </dev/null )
    sleep 4; pgrep -af qmlscene | cut -c1-120
  else
    [ -s $T/load.pid ] && kill "$(cat $T/load.pid)" 2>/dev/null || true
    rm -f $T/load.pid
  fi
  ;;
sample)
  # GPU time of the shell from DRM fdinfo (per drm-client-id), shell CPU,
  # load client frame rate, battery power (USB-powered phone: indicative only).
  s=$2; pid=$(pgrep -f "lomiri --mode=full" | head -1); B=/sys/class/power_supply/qcom-battmgr-bat
  gpu() { for f in /proc/$pid/fdinfo/*; do awk '/^drm-client-id/ {c=$2} /^drm-engine-gpu/ {print c, $2}' $f 2>/dev/null; done | sort -u | awk '{t+=$2} END {print t}'; }
  g0=$(gpu); t0=$(awk '{print $14+$15}' /proc/$pid/stat); f0=$(grep -c . $T/load.log 2>/dev/null || echo 0)
  e=0; n=0
  for i in $(seq 1 $s); do
    c=$(cat $B/current_now); v=$(cat $B/voltage_now)
    e=$(awk -v e=$e -v c=$c -v v=$v 'BEGIN{print e + (c<0?-c:c)/1e6*v/1e6}'); n=$((n+1)); sleep 1
  done
  g1=$(gpu); t1=$(awk '{print $14+$15}' /proc/$pid/stat)
  echo "shell GPU busy: $(awk -v a=$g0 -v b=$g1 -v s=$s 'BEGIN{printf "%.2f %% (%.2f ms per second)", (b-a)/s/1e7, (b-a)/s/1e6}')"
  echo "shell CPU: $(awk -v a=$t0 -v b=$t1 -v s=$s 'BEGIN{printf "%.1f %% of one core", (b-a)/s}')"
  echo "load client fps (last reports): $(grep fps $T/load.log 2>/dev/null | tail -2 | tr '\n' ' ')"
  echo "battery avg power: $(awk -v e=$e -v n=$n 'BEGIN{printf "%.3f W", e/n}') ($(cat $B/status), USB $(cat /sys/class/power_supply/qcom-battmgr-usb/online))"
  ;;
teardown)
  "$0" load stop || true
  systemctl --user stop fp6-dc-test 2>/dev/null || true
  dconf reset -f /com/fairphone6/display-comfort/
  cp -p $T/theme.ini.before ~/.config/lomiri-ui-toolkit/theme.ini
  if [ "$(cat $T/gtk3.before)" = none ]; then rm -f ~/.config/gtk-3.0/settings.ini; else cp -p $T/gtk3.before ~/.config/gtk-3.0/settings.ini; fi
  while IFS='=' read -r k v; do
    if [ -z "$v" ]; then dconf reset /org/gnome/desktop/interface/$k; else dconf write /org/gnome/desktop/interface/$k "$v"; fi
  done < $T/dconf.before
  gsettings set com.lomiri.Shell light-mode "$(cat $T/lightmode.before)"
  rm -f ~/.config/fp6-display-comfort/session.env; rmdir ~/.config/fp6-display-comfort 2>/dev/null || true
  rm -f $DROPIN; systemctl --user daemon-reload
  sudo_ umount $LOMIRI/share/lomiri/ShellScreen.qml || true
  findmnt | grep -c ShellScreen || true
  [ -s $P ] && kill "$(cat $P)" 2>/dev/null || true
  rm -f $P
  echo "teardown done at $(date +%T)"
  ;;
esac
