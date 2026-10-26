#!/bin/sh
# READ-ONLY. Measures wall-minus-RTC offset stability (the quantity the
# fp6-rtc-offset service saves). Default: 3 samples, 60 s apart (~2 min).
# If fp6-rtc-offset is installed in the booted generation, also prints its status.
# Power-off persistence test (MANUAL, needs user + reboot): after the module is
# active and NTP-synced once, power off for >= 10 min with Wi-Fi OFF at next
# boot; then 'fp6-rtc-offset status' and journalctl -u fp6-rtc-offset-restore.
. "$(dirname "$0")/misc-common.sh"
n=${1:-3}; gap=${2:-60}
i=0
while [ "$i" -lt "$n" ]; do
  ph 'r=$(cat /sys/class/rtc/rtc0/since_epoch); w=$(date -u +%s); echo "raw=$r wall=$w offset=$((w-r)) synced=$(timedatectl show -p NTPSynchronized --value)"'
  i=$((i+1)); [ "$i" -lt "$n" ] && sleep "$gap"
done
ph 'command -v fp6-rtc-offset >/dev/null && fp6-rtc-offset status && journalctl -b -u fp6-rtc-offset-restore -u fp6-rtc-offset-save --no-pager | tail -5 || echo "fp6-rtc-offset not installed in this generation"'
