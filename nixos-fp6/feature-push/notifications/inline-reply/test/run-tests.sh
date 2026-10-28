#!/usr/bin/env bash
# Offscreen laptop test + previews of inline replies in the shell.
#   nix-build test -o $SCRATCH/ir-env
#   nix-build test/patched-src.nix -o $SCRATCH/psrc   # the target's Lomiri tree, without this patch
#   test/run-tests.sh $SCRATCH/ir-env $SCRATCH/psrc OUTDIR
# Applies ../lomiri-inline-reply.patch (the working copy) to PATCHED_TREE's
# qml/, runs tst_InlineReply.qml on a private session bus with the patched
# x86 Lomiri.Notifications server, and sends its four notifications with
# fp6-notify-reply, checking what each client got. OUTDIR: screenshots + log.
set -eu
here=$(dirname "$(readlink -f "$0")")
if [ "${1:-}" != "--inner" ]; then
  out=$(readlink -f "$3"); mkdir -p "$out"
  exec "$1/bin/fp6-ir-env" sh -c 'exec "$FP6_DBUS_RUN_SESSION" --config-file="$1" -- bash "$0" --inner "$2" "$3"' \
    "$(readlink -f "$0")" "$here/session.conf" "$(readlink -f "$2")" "$out"
fi
tree=$2; out=$3
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
mkdir -p "$work/src"
cp -r "$tree"/qml "$work/src/"; chmod -R u+w "$work"
(cd "$work/src" && patch -p1 --no-backup-if-mismatch -s < "$here/../lomiri-inline-reply.patch")
cp "$here/tst_InlineReply.qml" "$work/src/qml/"
printf 'var out = "%s";\n' "$out" > "$work/src/qml/Shot.js"
# Components/Lockscreen.qml (SIM PIN snap decisions) imports Lomiri.Telephony
# without using it; nixpkgs has no x86 build of it on this path: an empty stub.
mkdir -p "$work/mocks/Lomiri/Telephony"
printf 'module Lomiri.Telephony\nStub 0.1 Stub.qml\n' > "$work/mocks/Lomiri/Telephony/qmldir"
echo 'import QtQml 2.15; QtObject {}' > "$work/mocks/Lomiri/Telephony/Stub.qml"
export QML2_IMPORT_PATH="$QML2_IMPORT_PATH:$work/mocks"

QT_QPA_PLATFORM=offscreen GRID_UNIT_PX=24 LANG=en_US.UTF-8 HOME=$work \
  timeout 400 "$FP6_QMLTESTRUNNER" -input "$work/src/qml/tst_InlineReply.qml" > "$work/qml.log" 2>&1 &
runner=$!
for _ in $(seq 200); do "$FP6_NOTIFY_REPLY" --check >/dev/null 2>&1 && break; sleep 0.1; done

fail=0
client() { # n, expected exit status, expected text in the JSON
  local o code
  set +e; o=$("$FP6_NOTIFY_REPLY" --json --wait 120 --summary "Test Contact $1" --body "Test message $1 from +999 555 010$1"); code=$?; set -e
  if [ "$code" = "$2" ] && printf '%s' "$o" | grep -qF "$3"; then echo "PASS client $1: exit $code $o"; else echo "FAIL client $1: exit $code $o (wanted $2, $3)"; fail=1; fi
}
{
  client 1 0 '"text":"On my way"'
  client 2 0 '"text":"Sent from the panel"'
  client 3 1 '"event":"closed"'
  client 4 0 '"text":"Reply while DND"'
} | tee "$work/clients.log"
grep -q FAIL "$work/clients.log" && fail=1
wait $runner || fail=1
grep -v "PERFORMANCE\|image://theme\|^$\|QDBus\|Powerd\|repowerd\|Could not register\|name already taken" "$work/qml.log" || true
cat "$work/clients.log" "$work/qml.log" > "$out/test-log.txt"
[ "$fail" = 0 ] && echo "ALL PASS" || { echo "FAILED"; exit 1; }
