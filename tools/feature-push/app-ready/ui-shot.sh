#!/usr/bin/env bash
# round.sh SETUP: System Settings > App Launch with Morph kept ready (opened
# through settings:///system/app-launch), DRM screenshots of the page.
# QML_DIR set: those page files are bind-mounted over the installed ones first.
set -u
here=$(dirname "$(readlink -f "$0")")
asroot() { SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p "" "$@"; }
export XDG_RUNTIME_DIR=/run/user/$(id -u)
LAL=$(dirname "$(ls /nix/store/*-lomiri-app-launch-*/bin/lomiri-app-launch | head -1)")
out=$here/out/ui; mkdir -p "$out"
shot() { asroot python3 "$here/drm-screenshot.py" > "$out/$1.png" 2>/dev/null; }
[ -n "${QML_DIR:-}" ] && "$here/qml-overlay.sh" on "$QML_DIR"
"$here/ar-test.sh" gs set com.fairphone6.app-ready status-file "$XDG_RUNTIME_DIR/fp6-app-ready/status.json"
"$here/ar-test.sh" gs set com.fairphone6.app-ready keep-ready "['morph-browser']"
"$here/ar-test.sh" gs set com.fairphone6.app-ready preload "['morph-browser']"
timeout 25 "$LAL/lomiri-app-launch" morph-browser >/dev/null 2>&1; sleep 12
timeout 25 "$LAL/lomiri-app-launch" lomiri-system-settings settings:///system/app-launch >/dev/null 2>&1; sleep 9
shot 01-app-launch
python3 "$here/touch.py" swipe 558 1900 558 700 400 >/dev/null; sleep 2
shot 02-app-launch-scrolled
python3 "$here/touch.py" swipe 558 1900 558 700 400 >/dev/null; sleep 2
shot 03-app-launch-bottom
journalctl --user --since -60s -o cat --no-pager | grep -i "qml-plugins/app-launch" > "$out/qml-errors.txt"
timeout 10 systemctl --user stop lomiri-app-launch--application-legacy--lomiri-system-settings--.service
[ -n "${QML_DIR:-}" ] && "$here/qml-overlay.sh" off "$QML_DIR"
