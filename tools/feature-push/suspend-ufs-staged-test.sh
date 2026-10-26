#!/bin/sh
# !!! MANUAL / USER-SUPERVISED ONLY - NEVER RUN FROM AN AGENT OR AUTOMATICALLY !!!
# Staged first suspend test for the milos UFS fix (kernel 7.2.0-fp6-nixos6,
# spm_lvl 5). See results/feature-push-20261024/suspend/README.md.
#
#   suspend-ufs-staged-test.sh check                      read-only preflight
#   suspend-ufs-staged-test.sh run <stage> --user-present  one cycle
#     stage: freezer | devices | platform   (pm_test, no real sleep)
#            s2idle                         (real s2idle, rtc wake +15 s)
#   suspend-ufs-staged-test.sh fetch                      copy on-phone logs
#
# A cycle runs on the phone as a transient systemd unit (survives the SSH or
# USB link dropping), syncs, writes kmsg markers, and saves the kernel log of
# the cycle to /var/log/fp6-suspend/ with fsync. It stops on the first UFS
# error. It bypasses systemd-sleep on purpose: the image keeps AllowSuspend=no
# so nothing else can suspend the phone.
. "$(dirname "$0")/misc-common.sh"
OUT_DIR=$FP6_ROOT/results/feature-push-20261024/suspend/runs; mkdir -p "$OUT_DIR"
U=/sys/bus/platform/devices/1d84000.ufshc
cmd=${1:-}; stage=${2:-}
case "$cmd" in
check)
  ph "set -u
  echo kernel=\$(uname -r)
  echo spm_lvl=\$(cat $U/spm_lvl) dev=\$(cat $U/spm_target_dev_state) link=\$(cat $U/spm_target_link_state)
  echo rpm_lvl=\$(cat $U/rpm_lvl)
  echo pm_test=\$(cat /sys/power/pm_test 2>/dev/null || echo MISSING) mem_sleep=\$(cat /sys/power/mem_sleep)
  echo waydroid=\$(systemctl is-active waydroid-container 2>/dev/null)
  echo capacity=\$(cat /sys/class/power_supply/qcom-battmgr-bat/capacity 2>/dev/null)
  echo journal=\$(ls -d /var/log/journal 2>/dev/null) panic=\$(cat /proc/sys/kernel/panic)
  echo rtc_wakeup=\$(cat /sys/class/rtc/rtc0/device/power/wakeup 2>/dev/null) rtc_name=\$(cat /sys/class/rtc/rtc0/name)
  echo h8fail_this_boot=\$($SUDO journalctl -b -k --no-pager | grep -c 'hibern8 exit failed')
  grep -H . /sys/power/suspend_stats/success /sys/power/suspend_stats/fail 2>/dev/null"
  echo "GO only if: kernel=7.2.0-fp6-nixos6 spm_lvl=5 link=OFF pm_test present waydroid inactive capacity>=50 h8fail=0";;
run)
  [ "${3:-}" = --user-present ] || { sed -n 2,17p "$0"; exit 2; }
  case "$stage" in freezer|devices|platform|s2idle) ;; *) echo "bad stage"; exit 2;; esac
  ts=$(stamp)
  PH_TIMEOUT=240 ph "set -u
  [ \"\$(uname -r)\" = 7.2.0-fp6-nixos6 ] || { echo 'REFUSE: not kernel6'; exit 3; }
  [ \"\$(cat $U/spm_lvl)\" = 5 ] || { echo 'REFUSE: spm_lvl != 5'; exit 3; }
  [ \"\$(systemctl is-active waydroid-container 2>/dev/null)\" != active ] || { echo 'REFUSE: stop Waydroid first'; exit 3; }
  $SUDO mkdir -p /var/log/fp6-suspend
  $SUDO systemd-run --unit=fp6-suspend-test-$ts --collect --wait --property=Type=oneshot --setenv=PATH=/run/current-system/sw/bin \
    /bin/sh -c '
    set -u; L=/var/log/fp6-suspend/$ts-$stage.txt; S=$stage
    log() { echo \"\$*\" >> \$L; }
    t0=\$(date +%s)
    log \"stage=\$S start=\$(date -Is) kernel=\$(uname -r) spm=\$(cat $U/spm_lvl)\"
    for w in /sys/class/power_supply/qcom-battmgr-*/power/wakeup; do echo disabled > \$w; done
    if [ \$S = s2idle ]; then echo none > /sys/power/pm_test; else echo \$S > /sys/power/pm_test; fi
    echo s2idle > /sys/power/mem_sleep
    journalctl --sync; sync; sync
    echo \"fp6-suspend-test: BEGIN \$S $ts\" > /dev/kmsg
    if [ \$S = s2idle ]; then
      rtcwake -m freeze -s 15 >> \$L 2>&1; rc=\$?
    else
      echo freeze > /sys/power/state; rc=\$?
    fi
    echo \"fp6-suspend-test: END \$S $ts rc=\$rc\" > /dev/kmsg
    echo none > /sys/power/pm_test
    for w in /sys/class/power_supply/qcom-battmgr-*/power/wakeup; do echo enabled > \$w; done
    # quick UFS I/O check after resume: write + fsync + read back
    dd if=/dev/urandom of=/var/log/fp6-suspend/io-probe bs=1M count=8 conv=fsync 2>>\$L
    sha=\$(sha256sum /var/log/fp6-suspend/io-probe | cut -c1-16); echo 3 > /proc/sys/vm/drop_caches
    sha2=\$(sha256sum /var/log/fp6-suspend/io-probe | cut -c1-16); rm -f /var/log/fp6-suspend/io-probe
    log \"rc=\$rc wall=\$((\$(date +%s)-t0))s io_probe=\$([ \$sha = \$sha2 ] && echo OK || echo MISMATCH)\"
    journalctl -k --no-pager -o short-monotonic --since=-5min | sed -n \"/BEGIN \$S $ts/,/END \$S $ts/p\" >> \$L
    log ufs_errors=\$(grep -cE \"hibern8 exit failed|ufshcd.*(failed|error|Reset)|host_reset|dev_reset|link startup failed\" \$L)
    grep -H . /sys/power/suspend_stats/success /sys/power/suspend_stats/fail /sys/power/suspend_stats/last_failed_dev >> \$L 2>/dev/null
    sync; sync'
  $SUDO cat /var/log/fp6-suspend/$ts-$stage.txt | grep -vE 'No record|DBG_RD|TSTBUS|host_regs|Vendor Specific'
  " | tee "$OUT_DIR/$ts-$stage.txt"
  if grep -qE '^ufs_errors=0$' "$OUT_DIR/$ts-$stage.txt" && grep -q 'io_probe=OK' "$OUT_DIR/$ts-$stage.txt"; then
    echo "PASS $stage"
  else
    echo "STOP: $stage did not pass cleanly. Do not continue to the next stage. Collect logs (fetch) and report."
  fi;;
fetch)
  ph "$SUDO tar -C /var/log -cf - fp6-suspend" | tar -C "$OUT_DIR" -xf - && ls -l "$OUT_DIR/fp6-suspend";;
*) sed -n 2,17p "$0"; exit 2;;
esac
