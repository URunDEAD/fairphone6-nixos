#!/usr/bin/env bash
# One GUI round on the phone (run as the phone user over the Wi-Fi ssh, since
# hosting re-enumerates USB), inside
#   flock -w 900 /run/user/10000/fp6-gui.lock phone-gui-round.sh APP CORE OUT SHOT_PY
# Uses the test helper on the private bus (phone-usb-test.sh setup). Temporary
# drawer entry + lomiri-app-launch, agent as a transient user unit, DRM
# screenshots (root, read-only). Restores screen lock state; removes the entry,
# stops app/agent, stops hosting. Well under the 10 min lock budget.
set -u
app=$1 core=$2 out=$3 shot=$4
lock=/run/user/10000/fp6-gui.lock
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
bus=unix:path=/run/fp6ud-test/bus
ual=$(nix-store -qR /run/current-system | grep -m1 -- '-lomiri-app-launch-[0-9]')
export PATH=$ual/bin:$core/bin:$PATH
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
cli() { FP6_USB_DRIVE_BUS_ADDRESS=$bus fp6-usb-drive-cli "$@" | cut -c1-100; }
ino=$(stat -c %i "$lock")
grep -q "FLOCK  ADVISORY  WRITE $PPID [0-9a-f:]*:$ino " /proc/locks || { echo "GUI lock NOT held by parent $PPID; abort"; exit 1; }
echo "GUI lock held (pid $PPID, inode $ino)"
mkdir -p "$out"
start=$(date +%s)
sess=$(loginctl list-sessions --no-legend | awk '$2==10000 && $4=="seat0"{print $1; exit}')
locked=$(loginctl show-session "$sess" -p LockedHint --value 2>/dev/null)
echo "session $sess locked=$locked"
gdbus call --system -d com.canonical.Unity.Screen -o /com/canonical/Unity/Screen \
  -m com.canonical.Unity.Screen.setScreenPowerMode on 1 >/dev/null 2>&1 || true
sleep 1
if [ "$locked" = yes ]; then asroot loginctl unlock-session "$sess"; sleep 2; fi
snap() { asroot python3 "$shot" > "$out/$1.png" 2>/dev/null; echo "shot $1"; }
entry=$HOME/.local/share/applications/fp6-usb-drive.desktop
launch() {
  printf '[Desktop Entry]\nType=Application\nName=USB Drive (test)\nExec=env FP6_USB_DRIVE_BUS_ADDRESS=%s FP6_PREVIEW=%s %s/bin/fp6-usb-drive\nIcon=%s/share/fp6-usb-drive/fp6-usb-drive.png\nTerminal=false\nX-Lomiri-Touch=true\n' \
    "$bus" "$1" "$app" "$app" > "$entry"
  lomiri-app-launch fp6-usb-drive 2>&1 | head -2 &
  sleep "${WAIT:-7}"
}
quit() {
  systemctl --user stop "lomiri-app-launch--application-legacy--fp6-usb-drive--*" 2>/dev/null
  sleep 1.5
  pkill -f "fp6-usb-drive-wrapped|bin/fp6-usb-drive$" 2>/dev/null
  rm -f "$entry"
}
mkdir -p "$(dirname "$entry")"
systemd-run --user -q --unit fp6ud-test-agent --collect -E FP6_USB_DRIVE_BUS_ADDRESS=$bus "$core/bin/fp6-usb-drive-agent"
sleep 1
launch ""
snap 01-library-idle
quit
cli host /home/user/Images/debian-13.7.0-amd64-netinst.iso cdrom
sleep 1.5
snap 02-notification
sleep 4
launch ""
snap 03-library-hosting
quit
launch host
snap 04-host-page
quit
cli stop
systemctl --user stop fp6ud-test-agent 2>/dev/null
pgrep -af 'fp6-usb-drive' | grep -v -e fp6-usb-drived -e phone-gui-round && echo "WARN leftover"
if [ "$locked" = yes ]; then asroot loginctl lock-session "$sess"; fi
echo "done in $(( $(date +%s) - start )) s"
