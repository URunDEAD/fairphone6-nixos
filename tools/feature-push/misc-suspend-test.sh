#!/bin/sh
# NOTE 2026-10-26: superseded by suspend-ufs-staged-test.sh (kernel6, spm_lvl 5). spm_lvl=2 cannot help:
# genpd cuts the UFS PHY GDSC in suspend_noirq regardless. See results/feature-push-20261024/suspend/README.md.
# !!! MANUAL / RISKY - NEVER RUN AUTOMATICALLY !!!
# One supervised suspend cycle to test the UFS hibern8-exit failure seen
# on 2026-10-24 (gsd-power idle suspends at 11:42 and 12:18 -> "hibern8 exit
# failed 5" + UFS resets). A failed UFS resume can kill the rootfs; only run
# with the phone on USB, Waydroid stopped, nothing important open, and a
# physical-button recovery path ready (RAM-boot recovery image).
# Steps: set spm_lvl=2 (link stays active; runtime value, reverts on reboot),
# disable battmgr psy wakeups, arm rtc wakealarm +30 s, 'systemctl suspend',
# collect UFS/PM logs, restore spm_lvl and wakeups.
# Requires: --i-accept-ufs-risk   Optional: --keep-default-spm (test as-is)
. "$(dirname "$0")/misc-common.sh"
[ "${1:-}" = --i-accept-ufs-risk ] || { sed -n 2,14p "$0"; exit 2; }
spm=2; [ "${2:-}" = --keep-default-spm ] && spm=keep
PH_TIMEOUT=180 ph "
set -u
U=/sys/bus/platform/devices/1d84000.ufshc
old=\$(cat \$U/spm_lvl); before=\$($SUDO journalctl -b -k --no-pager | grep -c 'hibern8 exit failed')
[ $spm = keep ] || echo $spm | $SUDO tee \$U/spm_lvl >/dev/null
for w in /sys/class/power_supply/qcom-battmgr-*/power/wakeup; do echo disabled | $SUDO tee \$w >/dev/null; done
echo 0 | $SUDO tee /sys/class/rtc/rtc0/wakealarm >/dev/null; echo +30 | $SUDO tee /sys/class/rtc/rtc0/wakealarm >/dev/null
t0=\$(date +%s); $SUDO systemctl suspend; sleep 45
echo slept_wall=\$((\$(date +%s)-t0)) spm_used=\$(cat \$U/spm_lvl)
echo \$old | $SUDO tee \$U/spm_lvl >/dev/null
for w in /sys/class/power_supply/qcom-battmgr-*/power/wakeup; do echo enabled | $SUDO tee \$w >/dev/null; done
echo hibern8_failures_new=\$((\$($SUDO journalctl -b -k --no-pager | grep -c 'hibern8 exit failed')-before))
grep -H . /sys/power/suspend_stats/success /sys/power/suspend_stats/fail /sys/power/suspend_stats/last_failed_dev 2>/dev/null
$SUDO journalctl -b -k --no-pager --since '-2min' | grep -E 'PM: |Wakeup pending|ufshcd|wakeup' | grep -v 'No record' | tail -25
" | tee "$OUT_DIR/suspend-test-$(stamp).txt"
