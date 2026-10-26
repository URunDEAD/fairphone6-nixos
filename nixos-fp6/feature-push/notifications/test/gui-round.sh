#!/usr/bin/env bash
# One GUI-lock round on the phone (as the phone user):
#   echo PIN | flock -w 900 /run/user/10000/fp6-gui.lock gui-round.sh LOMIRI_SESSION NOTIFYD INDICATOR OUT
# Test Lomiri via a runtime lomiri.service drop-in + one display-manager
# restart (relogin.sh, touch.py from tools/feature-push/app-ready, phone-type.py
# and tools/drm-screenshot.py as shot.py next to it), fp6-notifyd as a
# transient user unit, then: bubbles with DND off/on, allowed app, critical,
# no screen wake, indicator, bedtime dim. Teardown: daemon stopped, dconf
# keys reset, bubbles closed, drop-in removed, back on the system Lomiri.
set -u
LS=$1 ND=$2 IND=$3 OUT=$4 LSS=${5:-}
here=$(dirname "$(readlink -f "$0")")
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
read -r pin
echo "[$(date +%T)] lock held"
start=$(date +%s)
T=/tmp/fp6-dnd-test; rm -rf "$T"; mkdir -p "$T/cache" "$OUT"
K=/com/fairphone6/notifications
dropin=$XDG_RUNTIME_DIR/systemd/user/lomiri.service.d/zz-fp6-dnd-test.conf
dirs=$(systemctl --user show-environment | sed -n 's/^XDG_DATA_DIRS=//p')
shot() { asroot python3 "$here/shot.py" > "$OUT/$1.png" 2>/dev/null; echo "shot $1"; }
screen() { asroot gdbus call --system -d com.canonical.Unity.Screen -o /com/canonical/Unity/Screen \
  -m com.canonical.Unity.Screen.setScreenPowerMode "$1" 1 >/dev/null 2>&1 || true; }
bl() { echo "$(cat /sys/class/backlight/*/brightness | head -1)/$(cat /sys/class/drm/card0-DSI-1/dpms)"; }
# notify-send -w keeps the client connected (lomiri-notifications drops the
# bubbles of a client that disconnects, so one-shot gdbus calls never show).
NS=${NS:-$(ls -d /nix/store/*-libnotify-0.8.*/bin/notify-send 2>/dev/null | head -1)}
pids=""
notify() { # app summary body [urgency]; closes the previous test bubble first
  for p in $pids; do kill "$p" 2>/dev/null; done; pids=""; sleep 0.5
  "$NS" -a "$1" ${4:+-u "$4"} -w "$2" "$3" >/dev/null 2>&1 &
  pids="$pids $!"
}
relogin() {
  if [ "$1" = patched ]; then
    mkdir -p "$(dirname "$dropin")"
    printf '[Service]\nExecStart=\nExecStart=%s/bin/lomiri-session\nEnvironment=XDG_CACHE_HOME=%s/cache\nEnvironment="XDG_DATA_DIRS=%s/share:%s"\n' \
      "$LS" "$T" "$IND" "$dirs" > "$dropin"
  else
    rm -f "$dropin"; rmdir "$(dirname "$dropin")" 2>/dev/null
  fi
  systemctl --user daemon-reload
  echo "$pin" | "$here/relogin.sh"
}
restored=no
finish() {
  systemctl --user stop fp6-notifyd-test 2>/dev/null
  for p in $pids; do kill "$p" 2>/dev/null; done
  rm -f "$HOME/.local/share/applications/fp6-dnd-settings-test.desktop"
  dconf reset -f $K/
  rm -rf "$XDG_RUNTIME_DIR/fp6-notifyd" "$HOME/.local/share/fp6-notifyd" "$HOME/.local/state/fp6-notifyd"
  screen on
  if [ "$restored" = no ]; then relogin system || { asroot systemctl reset-failed display-manager; asroot systemctl start display-manager; }; restored=yes; fi
  rm -f "$dropin"; rmdir "$(dirname "$dropin")" 2>/dev/null; systemctl --user daemon-reload
  rm -rf "$T"
  echo "[$(date +%T)] teardown done; dropin: $(ls "$dropin" 2>&1 | tail -1); dconf: [$(dconf dump $K/)]; lomiri: $(systemctl --user show -p MainPID --value lomiri.service) $(readlink /proc/$(systemctl --user show -p MainPID --value lomiri.service)/exe)"
}
trap finish EXIT

relogin patched || exit 5
sleep 10
systemd-run --user -q --unit fp6-notifyd-test -p RuntimeMaxSec=540 "$ND/bin/fp6-notifyd"
sleep 3
dconf write $K/history-enabled true
journalctl --user -u lomiri.service --since "-60s" --no-pager | grep -iE "Notification|Wallpaper|GSettings|fp6|qml.*error" | head -20

echo "== 1 DND off: bubble shows"
screen on; sleep 1
notify fp6-test "DND off" "should show"; sleep 1.5; shot 01-dnd-off-bubble

echo "== 2 DND on: indicator icon, no bubble"
busctl --user call com.fairphone6.Dnd /com/fairphone6/Dnd com.fairphone6.Dnd SetActive b true; sleep 2
echo "dnd-active=$(dconf read $K/dnd-active)"
notify fp6-test "DND on" "should NOT show"; sleep 1.5; shot 02-dnd-on-no-bubble

echo "== 3 DND on: critical shows"
notify fp6-test "Critical" "should show" critical; sleep 1.5; shot 03-dnd-on-critical

echo "== 4 DND on: allowed app shows"
dconf write $K/dnd-allowed-apps "['fp6-allowed']"; sleep 1
notify fp6-allowed "Allowed app" "should show"; sleep 1.5; shot 04-dnd-on-allowed-app

echo "== 5 DND on: no screen wake"
screen off; sleep 3; b0=$(bl)
notify fp6-test "Screen off" "must not wake"; sleep 3; b1=$(bl)
echo "backlight before=$b0 after=$b1"
dconf write $K/dnd-hide-popups false; sleep 1
notify fp6-test "Screen off 2" "bubble kept, still no wake"; sleep 3; b2=$(bl)
echo "backlight (hide-popups off) after=$b2"
shot 05-screen-off-check; screen on; sleep 1; shot 05b-hide-popups-off-bubble-after-wake
dconf write $K/dnd-hide-popups true

echo "== 6 indicator menu state"
busctl --user call com.lomiri.indicator.dnd /com/lomiri/indicator/dnd org.gtk.Actions Describe s root

echo "== 7 bedtime: wallpaper dim (+ greyscale key)"
busctl --user call com.fairphone6.Dnd /com/fairphone6/Dnd com.fairphone6.Dnd SetBedtime b true; sleep 3
echo "bedtime-active=$(dconf read $K/bedtime-active) wallpaper-dim=$(dconf read $K/wallpaper-dim) greyscale=$(dconf read /com/fairphone6/display-comfort/greyscale-active)"
asroot loginctl lock-session $(loginctl list-sessions --no-legend | awk '$2==10000 && $4=="seat0"{print $1; exit}'); sleep 3
shot 07-bedtime-lockscreen-dimmed
asroot loginctl unlock-session $(loginctl list-sessions --no-legend | awk '$2==10000 && $4=="seat0"{print $1; exit}'); sleep 2
busctl --user call com.fairphone6.Dnd /com/fairphone6/Dnd com.fairphone6.Dnd SetBedtime b false; sleep 3
echo "after bedtime off: greyscale=$(dconf read /com/fairphone6/display-comfort/greyscale-active) dim=$(dconf read $K/wallpaper-dim)"
busctl --user call com.fairphone6.Dnd /com/fairphone6/Dnd com.fairphone6.Dnd SetActive b false; sleep 1
echo "== 8 history (titles only, test apps)"
python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print([(e["app"], e["summary"], e.get("body"), e.get("tags")) for e in d["history"]["entries"] if e["app"].startswith("fp6-")])' "$XDG_RUNTIME_DIR/fp6-notifyd/status.json"
ls -l "$HOME/.local/share/fp6-notifyd/"
if [ -n "$LSS" ]; then
  echo "== 9 Settings page"
  screen on
  ual=$(nix-store -qR /run/current-system | grep -m1 -- '-lomiri-app-launch-[0-9]')
  e=$HOME/.local/share/applications/fp6-dnd-settings-test.desktop
  printf '[Desktop Entry]\nType=Application\nName=DND settings (test)\nExec=%s/bin/lomiri-system-settings settings:///system/do-not-disturb\nIcon=night-mode\nTerminal=false\nX-Lomiri-Touch=true\n' "$LSS" > "$e"
  "$ual/bin/lomiri-app-launch" fp6-dnd-settings-test >/dev/null 2>&1 &
  sleep 8; shot 09-settings-dnd-page
  journalctl --user --since "-20s" --no-pager | grep -iE "do-not-disturb|DndModel" | cut -c1-250 | head -10 > "$T/lss.log"
  systemctl --user stop "lomiri-app-launch--application-legacy--fp6-dnd-settings-test--*" 2>/dev/null
  rm -f "$e"; sleep 1
  grep -iE "qml|error|warn" "$T/lss.log" | grep -iE "do-not-disturb|DndModel|Page" | head -10
fi
journalctl --user -u fp6-notifyd-test --no-pager | tail -15
journalctl --user -u lomiri.service --since "-6min" --no-pager | grep -iE "Notifications/|Wallpaper.qml|fp6" | head -20
echo "[$(date +%T)] tests done in $(( $(date +%s) - start )) s"
