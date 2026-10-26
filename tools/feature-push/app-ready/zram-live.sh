#!/bin/sh
# Live zram test (phone, root): zram-live.sh on|off|status
# on:  zstd, 50 % of RAM, priority 100, swappiness 100, page-cluster 0
#      (what app-ready-fp6.nix configures through zram-generator)
# off: swapoff, reset the device, restore swappiness 60 / page-cluster 3
set -eu
case ${1:-status} in
  on)
    grep -q '^/dev/zram0' /proc/swaps && exit 0
    echo 1 > /sys/block/zram0/reset
    echo zstd > /sys/block/zram0/comp_algorithm
    total_kb=$(awk '/^MemTotal:/ {print $2}' /proc/meminfo)
    echo $(( total_kb * 1024 / 2 )) > /sys/block/zram0/disksize
    mkswap /dev/zram0 >/dev/null
    swapon -p 100 /dev/zram0
    sysctl -q vm.swappiness=100 vm.page-cluster=0 ;;
  off)
    if grep -q '^/dev/zram0' /proc/swaps; then swapoff /dev/zram0; fi
    echo 1 > /sys/block/zram0/reset
    sysctl -q vm.swappiness=60 vm.page-cluster=3 ;;
esac
cat /proc/swaps; cat /sys/block/zram0/mm_stat 2>/dev/null || true
sysctl vm.swappiness vm.page-cluster
