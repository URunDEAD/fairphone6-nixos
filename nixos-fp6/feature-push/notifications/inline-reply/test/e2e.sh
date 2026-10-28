#!/usr/bin/env bash
# End-to-end D-Bus round on the laptop: a private session bus, the real
# (x86, patched) Lomiri.Notifications server in qmlscene (test/AutoReply.qml)
# and fp6-notify-reply as the client. Three rounds: reply while the bubble
# is up, reply after it timed out (parked: no NotificationClosed before the
# reply), and no reply (client times out).
#   nix-build test -o $SCRATCH/ir-env && test/e2e.sh $SCRATCH/ir-env
set -eu
here=$(dirname "$(readlink -f "$0")")
if [ "${1:-}" != "--inner" ]; then
  exec "$1/bin/fp6-ir-env" sh -c 'exec "$FP6_DBUS_RUN_SESSION" --config-file="$1" -- bash "$0" --inner' "$(readlink -f "$0")" "$here/session.conf"
fi
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
fail=0
run() { # mode, expected exit status, expected text in the client's JSON
  local mode=$1 want=$2 match=$3
  sed "s/property string mode: \"bubble\"/property string mode: \"$mode\"/" "$here/AutoReply.qml" > "$work/AutoReply-$mode.qml"
  QT_QPA_PLATFORM=offscreen "$FP6_QMLSCENE" "$work/AutoReply-$mode.qml" > "$work/server-$mode.log" 2>&1 &
  local srv=$!
  for _ in $(seq 100); do "$FP6_NOTIFY_REPLY" --check >/dev/null 2>&1 && break; sleep 0.1; done
  echo "== $mode: $("$FP6_NOTIFY_REPLY" --check || true)"
  local out code
  set +e; out=$("$FP6_NOTIFY_REPLY" --json --wait 15); code=$?; set -e
  kill $srv 2>/dev/null; wait $srv 2>/dev/null || true
  grep "e2e:" "$work/server-$mode.log" | sed "s/^.*e2e:/  [server]/" || true
  echo "  [client] exit $code: $out"
  if [ "$code" = "$want" ] && printf '%s' "$out" | grep -qF "$match"; then echo "PASS $mode"; else echo "FAIL $mode"; fail=1; fi
}
run bubble 0 '"text":"Auto reply ✓ from the test"'
run parked 0 '"text":"Late reply from the panel"'
run none 3 '"event":"timeout"'
exit $fail
