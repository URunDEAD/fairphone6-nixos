#!/usr/bin/env bash
# round.sh SETUP: Settings > App Launch screenshots, then the low-memory test.
set -u
here=$(dirname "$(readlink -f "$0")")
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
export XDG_RUNTIME_DIR=/run/user/$(id -u)
LAL=$(dirname "$(ls /nix/store/*-lomiri-app-launch-*/bin/lomiri-app-launch | head -1)")
out=$here/out/lmk-ui; mkdir -p "$out"
shot() { asroot python3 "$here/drm-screenshot.py" > "$out/$1.png" 2>/dev/null; }
launch() { timeout 25 "$LAL/lomiri-app-launch" "$@" >/dev/null 2>&1; }
gs() { "$here/ar-test.sh" gs set com.fairphone6.app-ready "$@"; }
gs keep-ready "['morph-browser']"; gs preload "['morph-browser']"
launch morph-browser; sleep 12
launch lomiri-system-settings settings:///system/app-launch; sleep 8
shot 01-app-launch
python3 "$here/touch.py" swipe 558 1600 558 700 400 >/dev/null; sleep 2
shot 02-app-launch-scrolled
cp "$XDG_RUNTIME_DIR/fp6-app-ready/status.json" "$out/status-before.json"
# Low memory: Morph kept ready (frozen), Messages + Settings plain background, Clock on screen.
launch lomiri-messaging-app; sleep 10
launch lomiri-clock-app; sleep 8
for p in lomiri-.*clock .lomiri-wrapped; do pgrep -u "$(id -u)" -f "$p" | head -1; done > "$out/pids-before.txt"
journalctl --user -u fp6-app-ready -n 30 -o cat --no-pager | grep oom_score > "$out/oom-before.txt"
STOP_AFTER=2 TIMEOUT=150 python3 "$here/hog.py" > "$out/hog.txt" 2>&1
sleep 3
cp "$XDG_RUNTIME_DIR/fp6-app-ready/status.json" "$out/status-after.json"
systemctl --user list-units --no-legend --plain "lomiri-app-launch*" > "$out/units-after.txt"
journalctl -b -u earlyoom --since -5min -o cat --no-pager > "$out/earlyoom.txt"
asroot dmesg | grep -i -E "out of memory|oom-kill|Killed process" | tail -5 > "$out/kernel-oom.txt"
cat /proc/swaps > "$out/swaps-after.txt"
tail -4 "$out/hog.txt"; cat "$out/units-after.txt" | cut -c1-90
