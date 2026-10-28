#!/usr/bin/env bash
# Memory & performance benchmark, laptop side. Run it with the phone owner
# present (it opens and closes apps on the phone's screen and drops the
# page cache; nothing else is changed).
#   tools/feature-push/memory-performance/bench.sh OUT [APPS...]
#   tools/feature-push/memory-performance/bench.sh --snapshot   (read-only RAM picture)
# Copies snapshot.sh, bench-phone.sh and app-ready's measure.sh to
# ~/fp6-bench on the phone (over ssh stdin) and runs bench-phone.sh there.
# Env: MODES (default "cold warm resume prelaunch"), FP6_SSH_ADDRESS / FP6_SSH_USB
# as for tools/phone-ssh. The phone password is read by tools/phone-ssh
# from .fp6-phone-secret; it is never put on a command line.
set -eu
here=$(dirname "$(readlink -f "$0")")
root=$(readlink -f "$here/../../..")
ssh=$root/tools/phone-ssh
if [ "${1:-}" = --snapshot ]; then exec "$ssh" 'sh -s' < "$here/snapshot.sh"; fi
out=$(readlink -f "${1:?OUT dir}"); shift
mkdir -p "$out"
for f in "$here/snapshot.sh" "$here/bench-phone.sh" "$root/tools/feature-push/app-ready/measure.sh"; do
  "$ssh" "mkdir -p ~/fp6-bench && cat > ~/fp6-bench/$(basename "$f")" < "$f"
done
name=bench-$(date +%Y%m%d-%H%M%S)
"$ssh" "MODES='${MODES:-cold warm resume prelaunch}' bash ~/fp6-bench/bench-phone.sh ~/fp6-bench/$name $*" | tee "$out/$name.log"
"$ssh" "cd ~/fp6-bench && tar cf - $name" | tar xf - -C "$out"
echo "results: $out/$name"
