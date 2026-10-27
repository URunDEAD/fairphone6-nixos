#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --locked
root=$PWD
export WEBSHARE_TEST_ROOT="$root"
dbus-run-session -- bash <<'INNER'
set -euo pipefail
work=$(mktemp -d)
export XDG_RUNTIME_DIR="$work/runtime" XDG_DATA_HOME="$work/data"
mkdir -m 700 "$XDG_RUNTIME_DIR" "$XDG_DATA_HOME"
bin="$WEBSHARE_TEST_ROOT/target/debug/fp6-webshare"
"$bin" daemon > "$work/daemon.log" 2>&1 &
service=$!
trap 'kill "$service" 2>/dev/null || true; wait "$service" 2>/dev/null || true; rm -rf "$work"' EXIT
for i in $(seq 1 100); do
  if "$bin" status > "$work/status.json" 2>/dev/null; then break; fi
  sleep .05
done
python3 - "$work/status.json" "$XDG_RUNTIME_DIR/fp6-webshare/control.json" <<'PY'
import json, os, stat, sys
assert json.load(open(sys.argv[1]))['active'] is False
assert stat.S_IMODE(os.stat(sys.argv[2]).st_mode) == 0o600
PY
"$bin" interfaces > "$work/interfaces.json"
python3 - "$work/interfaces.json" <<'PY'
import json, sys
assert isinstance(json.load(open(sys.argv[1])), list)
PY
# A second daemon must not replace the first, queue for its name, or touch staging.
printf synthetic > "$XDG_RUNTIME_DIR/fp6-webshare/staging/protected"
if timeout 5 "$bin" daemon > "$work/second.log" 2>&1; then exit 1; fi
test -f "$XDG_RUNTIME_DIR/fp6-webshare/staging/protected"
"$bin" status > /dev/null
if "$bin" share --interface lo --uploads > "$work/rejected.log" 2>&1; then exit 1; fi
if "$bin" approve unknown > /dev/null 2>&1; then exit 1; fi
"$bin" stop
kill -TERM "$service"
wait "$service"
test ! -f "$XDG_RUNTIME_DIR/fp6-webshare/control.json"
# A later clean startup also removes crash leftovers.
"$bin" daemon > "$work/restart.log" 2>&1 &
service=$!
for i in $(seq 1 100); do
  if "$bin" status > /dev/null 2>&1; then break; fi
  sleep .05
done
test ! -f "$XDG_RUNTIME_DIR/fp6-webshare/staging/protected"
echo 'D-Bus lifecycle, duplicate daemon, private control file and crash cleanup: passed'
INNER
