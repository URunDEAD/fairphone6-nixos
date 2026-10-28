#!/usr/bin/env bash
# Laptop tests of the FP6 lock sounds (no phone):
#   nix-build test -o $SCRATCH/ls-env
#   test/run-tests.sh $SCRATCH/ls-env OUTDIR
#  1. tst_LockSounds.qml (qmltestrunner, offscreen) with the patched shell
#     QML on a private bus with test/mock-bus.py;
#  2. fp6-lock-sounds sync against real gsettings (memory backend is per
#     process, so a keyfile backend in a temporary XDG_CONFIG_HOME);
#  3. test/audio-e2e.sh: PipeWire + pipewire-pulse + WirePlumber, the
#     SoundEffect stream's media.role and the play latency;
#  4. Settings previews (test/preview.sh).
# OUTDIR: logs and screenshots.
set -eu
here=$(dirname "$(readlink -f "$0")")
env=$1/bin/fp6-ls-env
out=$(readlink -f "$2"); mkdir -p "$out"
fail=0

if [ "${3:-}" != "--inner" ]; then
  # 1. shell QML
  work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
  "$env" sh -c 'cp "$FP6_LS_LOMIRI_QML"/Components/Fp6LockSound* "$1"/ && sed "s|@BUNDLE@|$FP6_LS_BUNDLE|g" "$2" > "$1/tst_LockSounds.qml"' \
    _ "$work" "$here/tst_LockSounds.qml"
  chmod -R u+w "$work"
  "$env" sh -c 'exec "$FP6_DBUS_RUN_SESSION" -- bash "$0" "$1" "$2" --inner "$3"' "$(readlink -f "$0")" "$1" "$out" "$work" \
    > "$out/shell-test.log" 2>&1 || fail=1
  grep -E "^(PASS|FAIL|Totals)|^   (Actual|Expected|Loc)" "$out/shell-test.log" || true

  # 2. sync with gsettings
  if "$env" bash "$here/sync-test.sh" > "$out/sync-test.log" 2>&1; then echo "PASS sync-test"; else echo "FAIL sync-test"; fail=1; fi
  tail -n 12 "$out/sync-test.log"

  # 3. audio
  if "$env" bash "$here/audio-e2e.sh" "$out" > "$out/audio-e2e.log" 2>&1; then echo "PASS audio-e2e"; else echo "FAIL audio-e2e"; fail=1; fi
  grep -E "PASS|FAIL|latency|media.role" "$out/audio-e2e.log" || true

  # 4. Settings previews
  "$env" bash "$here/preview.sh" "$out" > "$out/preview.log" 2>&1 || fail=1
  grep -E "PASS|FAIL" "$out/preview.log" || true

  [ "$fail" = 0 ] && echo "ALL PASS" || { echo "FAILED"; exit 1; }
  exit 0
fi

# --inner (on the private bus): the bus is also the "system" bus for Powerd.
work=$4
export DBUS_SYSTEM_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS
"$FP6_PYTHON" "$here/mock-bus.py" > "$work/mock.out" & mock=$!
for _ in $(seq 100); do grep -q ready "$work/mock.out" && break; sleep 0.1; done
set +e
QT_QPA_PLATFORM=offscreen HOME=$work timeout 300 "$FP6_QMLTESTRUNNER" -input "$work/tst_LockSounds.qml"
rc=$?
kill $mock
exit $rc
