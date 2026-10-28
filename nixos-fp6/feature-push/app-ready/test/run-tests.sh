#!/usr/bin/env bash
# Laptop tests of Memory & performance (no phone):
#   nix-build test -o $SCRATCH/mp-env      (cargo test, qmllint of the plugin
#                                           and of the patched Stage.qml)
#   test/run-tests.sh $SCRATCH/mp-env OUTDIR
#  1. fp6-app-ready CLI: profile table, apply-profile refuses an unknown name,
#     clear-history without a daemon (temporary HOME);
#  2. Settings previews (test/preview.sh).
set -eu
here=$(dirname "$(readlink -f "$0")")
env=$1/bin/fp6-mp-env
out=$(readlink -f "$2"); mkdir -p "$out"
fail=0
{
  home=$(mktemp -d)
  "$env" sh -c '
    set -e
    for p in conservative balanced aggressive; do "$FP6_APP_READY" profile $p; done
    if "$FP6_APP_READY" apply-profile turbo; then echo "FAIL apply-profile accepted turbo"; exit 1; fi
    mkdir -p "$1/.local/state/fp6-app-ready" "$1/.cache/fp6-app-ready"
    echo "fp6-app-ready usage v1" > "$1/.local/state/fp6-app-ready/usage"
    echo /bin/sh > "$1/.cache/fp6-app-ready/x.files"
    HOME=$1 XDG_STATE_HOME= XDG_CACHE_HOME= DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent "$FP6_APP_READY" clear-history
    test ! -e "$1/.local/state/fp6-app-ready/usage" && test ! -e "$1/.cache/fp6-app-ready/x.files"
    echo "PASS cli"
    grep -c "fp6AppReady.predicted" "$FP6_MP_STAGE" >/dev/null && echo "PASS patched Stage.qml (qmllint in the build)"
  ' _ "$home"
  rm -rf "$home"
} > "$out/cli.txt" 2>&1 || fail=1
cat "$out/cli.txt"
"$env" bash "$here/preview.sh" "$out" > "$out/settings-checks.txt" 2>&1 || fail=1
grep -E "PASS|FAIL|checks:" "$out/settings-checks.txt" || true
exit $fail
