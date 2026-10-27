#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
bin_dir=${FP6_BIN_DIR:-$repo/target/debug}
if [[ ${1:-} != --session ]]; then
  exec dbus-run-session --config-file="$repo/tests/session-bus.conf" -- "$0" --session
fi
scratch=$(mktemp -d)
cleanup() {
  if [[ -n ${daemon_pid:-} ]]; then kill "$daemon_pid" 2>/dev/null || true; wait "$daemon_pid" 2>/dev/null || true; fi
  rm -rf "$scratch"
}
trap cleanup EXIT
"$bin_dir/fp6-quickshare-daemon" --interfaces lo --port 0 --download-dir "$scratch/received" >"$scratch/daemon.log" 2>&1 &
daemon_pid=$!
for attempt in {1..100}; do
  if busctl --user status com.fairphone6.QuickShare1 >/dev/null 2>&1; then break; fi
  sleep 0.05
done
busctl --user introspect com.fairphone6.QuickShare1 /com/fairphone6/QuickShare1 com.fairphone6.QuickShare1 > "$scratch/api"
for member in Visibility DeviceName State StartDiscovery StopDiscovery Send Accept Reject Cancel DeviceFound DeviceLost IncomingRequest Progress Finished; do
  rg -q "\.${member} " "$scratch/api"
done
"$bin_dir/fp6-quickshare" status | rg 'visibility: off'
busctl --user set-property com.fairphone6.QuickShare1 /com/fairphone6/QuickShare1 com.fairphone6.QuickShare1 Visibility s everyone-for-10-min
"$bin_dir/fp6-quickshare" status | rg 'everyone-for-10-min'
if busctl --user set-property com.fairphone6.QuickShare1 /com/fairphone6/QuickShare1 com.fairphone6.QuickShare1 Visibility s contacts 2>/dev/null; then echo 'contacts unexpectedly accepted' >&2; exit 1; fi
if "$bin_dir/fp6-quickshare" accept nonexistent 2>/dev/null; then echo 'invalid consent unexpectedly accepted' >&2; exit 1; fi
busctl --user set-property com.fairphone6.QuickShare1 /com/fairphone6/QuickShare1 com.fairphone6.QuickShare1 DeviceName s 'Synthetic test phone'
"$bin_dir/fp6-quickshare" devices --wait 0
"$bin_dir/fp6-quickshare" status | rg 'Synthetic test phone: discovering'
busctl --user call com.fairphone6.QuickShare1 /com/fairphone6/QuickShare1 com.fairphone6.QuickShare1 StopDiscovery
busctl --user set-property com.fairphone6.QuickShare1 /com/fairphone6/QuickShare1 com.fairphone6.QuickShare1 Visibility s off
"$bin_dir/fp6-quickshare" status | rg 'visibility: off'
kill "$daemon_pid"
wait "$daemon_pid"
daemon_pid=
"$bin_dir/fp6-quickshare-daemon" --interfaces lo --port 0 --download-dir "$scratch/received" >"$scratch/daemon.log" 2>&1 &
daemon_pid=$!
for attempt in {1..100}; do
  if "$bin_dir/fp6-quickshare" status 2>/dev/null | rg -q 'visibility: off'; then break; fi
  sleep 0.05
done
"$bin_dir/fp6-quickshare" status | rg 'visibility: off'
printf '%s\n' 'D-Bus properties, signals, CLI, invalid consent, discovery and restart smoke tests passed.'
