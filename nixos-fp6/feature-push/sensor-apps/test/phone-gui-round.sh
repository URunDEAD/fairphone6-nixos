#!/usr/bin/env bash
# One GUI test round on the phone (run as the phone user, inside
#   flock -w 900 /run/user/10000/fp6-gui.lock phone-gui-round.sh PKG OUT TOOL[:PREVIEW] ...
# ). For each app: temporary drawer entry + lomiri-app-launch, wait, DRM
# screenshot (root, read-only), stop, remove the entry. Restores the screen
# power / lock state it found. Keeps well under the 10 min lock budget.
set -u
pkg=$1 out=$2; shift 2
dir=$(dirname "$0")
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
# lomiri-app-launch is not on the user PATH on this system
ual=$(nix-store -qR /run/current-system | grep -m1 -- '-lomiri-app-launch-[0-9]')
export PATH=$ual/bin:$PATH
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
mkdir -p "$out"
start=$(date +%s)

sess=$(loginctl list-sessions --no-legend | awk '$2==10000 && $4=="seat0"{print $1; exit}')
locked=$(loginctl show-session "$sess" -p LockedHint --value 2>/dev/null)
echo "session $sess locked=$locked"
# display on (powerd's Unity.Screen API, as the power key does)
gdbus call --system -d com.canonical.Unity.Screen -o /com/canonical/Unity/Screen \
  -m com.canonical.Unity.Screen.setScreenPowerMode on 1 >/dev/null 2>&1 || true
sleep 1
if [ "$locked" = yes ]; then asroot loginctl unlock-session "$sess"; sleep 2; fi
asroot python3 "$dir/shot.py" > "$out/00-before.png" 2>/dev/null

loc() { gdbus call --session -d com.lomiri.indicator.location -o /com/lomiri/indicator/location \
  -m org.gtk.Actions.SetState location-detection-enabled "<$1>" '{}' >/dev/null; }
for spec in "$@"; do
  [ $(( $(date +%s) - start )) -gt 480 ] && { echo "time budget reached"; break; }
  tool=${spec%%:*}; preview=${spec#*:}; [ "$preview" = "$spec" ] && preview=
  id=fp6-$tool
  entry=$HOME/.local/share/applications/$id.desktop
  mkdir -p "$(dirname "$entry")"
  printf '[Desktop Entry]\nType=Application\nName=%s (test)\nExec=env FP6_PREVIEW=%s %s/bin/fp6-sensor-tools %s\nIcon=%s/share/fp6-sensor-apps/icons/%s.png\nTerminal=false\nX-Lomiri-Touch=true\n' \
    "$tool" "$preview" "$pkg" "$tool" "$pkg" "$id" > "$entry"
  if [ "$preview" = locon ]; then
    loc true; echo "location on"
    # test-only stand-in for the module's fp6-gnss-skyfeed.socket
    if [ -n "${SKYFEED:-}" ]; then
      asroot systemd-run -q --unit fp6-skyfeed-test -p RuntimeMaxSec=120 -p RuntimeDirectory=fp6-gnss-sky -p RuntimeDirectoryMode=0755 \
        "${SOCAT:-socat}" UNIX-LISTEN:/run/fp6-gnss-sky/sky.sock,fork,unlink-early,mode=0660,group=radio \
        "EXEC:/run/current-system/sw/bin/systemd-run -q --pipe -p DynamicUser=yes -p SupplementaryGroups=geoclue -p RuntimeMaxSec=100 $SKYFEED"
    fi
  fi
  lomiri-app-launch "$id" 2>&1 | head -3 &
  if [ "$preview" = locon ]; then sleep 20; else sleep "${WAIT:-6}"; fi
  name=$tool${preview:+-$preview}
  asroot python3 "$dir/shot.py" > "$out/$name.png" 2>/dev/null
  pgrep -af "fp6-sensor-tools(-wrapped)? $tool" | cut -c1-120 | head -2
  # lomiri-app-stop loses its registry connection here; stop the app's
  # systemd user unit (what UAL started) instead
  systemctl --user stop "lomiri-app-launch--application-legacy--$id--*" 2>/dev/null
  sleep 1.5
  pkill -f "fp6-sensor-tools(-wrapped)? $tool" 2>/dev/null
  pgrep -f "fp6-sensor-tools(-wrapped)? $tool" >/dev/null && echo "WARN $tool still running"
  pgrep -f "pw-record" >/dev/null && echo "WARN pw-record still running"
  rm -f "$entry"
  if [ "$preview" = locon ]; then
    [ -n "${SKYFEED:-}" ] && asroot systemctl stop fp6-skyfeed-test 2>/dev/null
    loc false; sleep 2; echo "location off: $(cat ~/.config/fp6-location/enabled)"; fi
done
if [ "$locked" = yes ]; then asroot loginctl lock-session "$sess"; fi
echo "done in $(( $(date +%s) - start )) s"
