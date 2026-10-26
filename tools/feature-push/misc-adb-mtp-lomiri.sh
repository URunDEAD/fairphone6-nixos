#!/bin/sh
# READ-ONLY (default): ADB/MTP are system services independent of the UI
# session; this reports the gadget, adbd and fp6-mtp plus the active session
# type (phosh/lomiri) and host-side enumeration.
# --start-mtp / --stop-mtp: REVERSIBLE (fp6-mtp restores NCM+ADB on exit).
#   USB re-enumerates for ~2 s; SSH over USB may drop briefly.
. "$(dirname "$0")/misc-common.sh"
case "${1:-}" in
  --start-mtp) ph "$SUDO systemctl start fp6-mtp; sleep 5; systemctl is-active fp6-mtp" ;;
  --stop-mtp) ph "$SUDO systemctl stop fp6-mtp; systemctl is-active fp6-mtp || true" ;;
esac
ph 'loginctl show-session $(loginctl list-sessions --no-legend | awk "\$4==\"seat0\"{print \$1}") -p Type -p Desktop 2>/dev/null; systemctl --user is-active lomiri phosh 2>/dev/null | tr "\n" " "; echo; systemctl is-active adbd fp6-mtp | tr "\n" " "; echo; ls /sys/kernel/config/usb_gadget/g1/configs/c.1/'
command -v adb >/dev/null && timeout 10 adb devices -l || echo "adb not on host PATH"
lsusb 2>/dev/null | grep -i -E '18d1|fairphone' || true
