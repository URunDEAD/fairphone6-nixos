#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ ${1:-} != --inside ]]; then
    cargo build --locked --bins --examples
    exec dbus-run-session -- "$0" --inside
fi
pids=()
cleanup() { for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done; wait || true; }
trap cleanup EXIT
target/debug/fp6-callsd --demo & pids+=("$!")
target/debug/examples/synthetic-shell & pids+=("$!")
# Wait for D-Bus ownership rather than assuming a fixed launch time.
for attempt in {1..100}; do
    if dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:com.fairphone6.Calls1 | grep -q 'boolean true'; then break; fi
    sleep 0.05
done
target/debug/fp6-voip-demo & pids+=("$!")
wait "${pids[1]}"
wait "${pids[2]}"
