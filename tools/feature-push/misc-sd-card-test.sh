#!/bin/sh
# Phase 1 READ-ONLY (default): waits up to $1 s (default 120) while the USER
#   inserts a microSD card, then reports enumeration, partitions, filesystem.
#   Card insertion is a user action; nothing is mounted.
# Phase 2 (--mount, needs fp6SdAutomount in the booted generation): reports
#   what the udev-triggered fp6-sd-mount@ did. Reversible: 'sudo umount <dir>'
#   or simply remove the card (mount unit is BindsTo the device).
. "$(dirname "$0")/misc-common.sh"
case "${1:-}" in
  --mount)
    ph "systemctl list-units --no-legend 'fp6-sd-mount@*' 'media-*.mount'; findmnt -rn -o TARGET,SOURCE,FSTYPE,OPTIONS | grep '^/media/' || echo 'nothing under /media'; $SUDO journalctl -b -u 'fp6-sd-mount@*' --no-pager | tail -10"
    exit 0 ;;
esac
wait=${1:-120}
echo "Insert the microSD card now (waiting up to ${wait}s)..."
PH_TIMEOUT=$((wait+30)) ph "
t=0; while [ \$t -lt $wait ]; do ls /sys/bus/mmc/devices/ 2>/dev/null | grep -q . && break; sleep 2; t=\$((t+2)); done
echo gpio65_cd=\$($SUDO grep 'gpio65 ' /sys/kernel/debug/gpio)
ls /sys/bus/mmc/devices/ 2>/dev/null || true
for d in /sys/bus/mmc/devices/*; do [ -e \"\$d\" ] && grep -H . \$d/type \$d/name \$d/date 2>/dev/null; done
lsblk -o NAME,SIZE,FSTYPE,LABEL,MOUNTPOINTS /dev/mmcblk0 2>&1
udevadm info -q property -n /dev/mmcblk0p1 2>/dev/null | grep -E 'ID_PATH=|ID_FS_TYPE|ID_FS_USAGE' || true
$SUDO journalctl -k -b --no-pager | grep -E 'mmc0|mmcblk' | tail -8
"
