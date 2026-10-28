#!/bin/sh
# Read-only RAM picture of the phone (phone side, as the phone user): what
# Settings > Memory & performance summarises, plus the kernel's view.
# Changes nothing. Used by bench-phone.sh before/after each round, and on
# its own:  tools/phone-ssh 'sh -s' < tools/feature-push/memory-performance/snapshot.sh
export XDG_RUNTIME_DIR=/run/user/$(id -u)
echo "== $(date -Is) up $(cut -d' ' -f1 /proc/uptime)s load $(cut -d' ' -f1-3 /proc/loadavg)"
echo "== system $(readlink /run/current-system)"
awk '/^(MemTotal|MemFree|MemAvailable|Buffers|Cached|SwapCached|Active\(anon\)|Inactive\(anon\)|Active\(file\)|Inactive\(file\)|Unevictable|Mlocked|SwapTotal|SwapFree|AnonPages|Mapped|Shmem|SReclaimable):/ {printf "%-16s %8d MiB\n", $1, $2/1024}' /proc/meminfo
echo "== zram"; cat /proc/swaps | tail -n +2
[ -r /sys/block/zram0/mm_stat ] && awk '{printf "stored %d MiB, compressed %d MiB, in RAM %d MiB, ratio %.2f\n", $1/1048576, $2/1048576, $3/1048576, ($2>0? $1/$2 : 0)}' /sys/block/zram0/mm_stat
echo "== psi memory"; cat /proc/pressure/memory
echo "== psi io"; cat /proc/pressure/io
echo "== vm"; for k in swappiness vfs_cache_pressure watermark_scale_factor watermark_boost_factor page-cluster min_free_kbytes; do printf "%s=%s " $k "$(cat /proc/sys/vm/$k)"; done; echo
for l in /sys/block/loop*; do [ -e $l/loop/backing_file ] && echo "$(basename $l) dio=$(cat $l/loop/dio) ra=$(cat $l/queue/read_ahead_kb)"; done
[ -e /sys/kernel/mm/lru_gen/enabled ] && echo "MGLRU $(cat /sys/kernel/mm/lru_gen/enabled) min_ttl_ms=$(cat /sys/kernel/mm/lru_gen/min_ttl_ms)" || echo "MGLRU: not in kernel"
echo "== vmstat"; grep -E "^(pgmajfault|pswpin|pswpout|workingset_refault_file|workingset_refault_anon|pgsteal_kswapd|pgsteal_direct|allocstall_normal|oom_kill) " /proc/vmstat | tr '\n' ' '; echo
echo "== apps (cgroup memory / swap / state)"
slice=/sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/app.slice
for d in "$slice"/lomiri-app-launch--application-*; do
  [ -d "$d" ] || continue
  st=running; for p in $(cat "$d/cgroup.procs"); do grep -q '^State:.*T' /proc/$p/status 2>/dev/null && st=frozen; done
  printf "%-40s %6d MiB %6d MiB %s\n" "$(basename "$d" | sed 's/^lomiri-app-launch--application-[a-z]*--//; s/--[^-]*\.service$//; s/--\.service$//')" \
    $(( $(cat "$d/memory.current") / 1048576 )) $(( $(cat "$d/memory.swap.current" 2>/dev/null || echo 0) / 1048576 )) $st
done
p=$(ps -eo pid,comm | awk '$2==".lomiri-wrapped"{print $1; exit}')
[ -n "$p" ] && echo "lomiri majflt $(awk '{print $12}' /proc/$p/stat) rss $(awk '/VmRSS/ {print $2/1024 " MiB"}' /proc/$p/status)"
echo "== fp6-app-ready"
f=$XDG_RUNTIME_DIR/fp6-app-ready/status.json
if [ -r "$f" ]; then
  python3 - "$f" <<'PY' 2>/dev/null || head -c 600 "$f"
import json, sys
d = json.load(open(sys.argv[1]))
print("profile", d.get("profile"), "applied", d.get("profileApplied"), "prediction", d.get("prediction"), "readyApps", d.get("readyApps"))
print("ram", {k: v >> 20 for k, v in (d.get("ram") or {}).items()}, "MiB")
for a in d.get("predicted", []):
    print("  predicted %-28s %5.1f %%  ready=%s running=%s frozen=%s" % (a["id"], 100 * a["p"], a["ready"], a["running"], a["frozen"]))
print("hits", d.get("hits"))
print("kills", d.get("lowMemoryKills"))
PY
else echo "no status file"; fi
