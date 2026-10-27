#!/usr/bin/env bash
# Tests for fp6-rmtfs-shadow (attempt counter, 2-start fallback, reset,
# scratch link) against a temporary directory and a fake qmicli.
# Usage: test-fp6-rmtfs-shadow.sh <path to fp6-rmtfs-shadow>
# shellcheck disable=SC2016  # check() evals its single-quoted conditions
set -euo pipefail
TOOL=$1
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
export FP6_RMTFS_SHADOW_DIR=$T/shadow FP6_TQFTP_SCRATCH=$T/run-tqftpserv FP6_RMTFS_SHADOW_CONFIRM_WAIT=1
mkdir -p "$T/bin"
cat > "$T/bin/qmicli" <<'EOF'
#!/bin/sh
echo "	Status:      ${FAKE_PDC_STATUS:-Active}"
EOF
chmod +x "$T/bin/qmicli"
export FP6_RMTFS_SHADOW_QMICLI=$T/bin/qmicli
D=$FP6_RMTFS_SHADOW_DIR
fail=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }
counter() { cat "$D/boot-attempts"; }

"$TOOL" prestart >/dev/null
check "fresh: dir 0700" '[ "$(stat -c %a "$D")" = 700 ]'
check "fresh: counter 0" '[ "$(counter)" = 0 ]'
check "fresh: scratch linked" '[ "$(readlink "$FP6_TQFTP_SCRATCH")" = "$D/readwrite" ] && [ -d "$D/readwrite" ]'
echo stale > "$D/readwrite/mcfg.tmp"
"$TOOL" prestart >/dev/null
check "fresh EFS gets a fresh scratch" '[ ! -e "$D/readwrite/mcfg.tmp" ]'

# rmtfs saved images during that boot
echo img > "$D/modemst1.img"; echo img > "$D/modemst2.img"; echo tmp > "$D/modemst1.img.tmp"
echo keep > "$D/readwrite/mcfg.tmp"
"$TOOL" prestart >/dev/null
check "attempt 1 counted" '[ "$(counter)" = 1 ]'
check "scratch kept with images" '[ -e "$D/readwrite/mcfg.tmp" ]'
check "stale tmp removed" '[ ! -e "$D/modemst1.img.tmp" ]'
"$TOOL" confirm >/dev/null
check "confirm (Active) clears counter" '[ "$(counter)" = 0 ]'

# two unconfirmed starts -> fallback on the third
"$TOOL" prestart >/dev/null
FAKE_PDC_STATUS=Pending "$TOOL" confirm >/dev/null
check "unconfirmed keeps counter 1" '[ "$(counter)" = 1 ]'
"$TOOL" prestart >/dev/null
check "attempt 2 counted" '[ "$(counter)" = 2 ]'
check "images still loaded on attempt 2" '[ -e "$D/modemst1.img" ]'
out=$("$TOOL" prestart)
check "fallback announced" 'grep -q FALLBACK <<<"$out"'
check "fallback: images moved away" '[ ! -e "$D/modemst1.img" ] && [ ! -e "$D/modemst2.img" ]'
check "fallback: quarantined" '[ -n "$(ls "$D/quarantine"/*/modemst1.img 2>/dev/null)" ]'
check "fallback: counter 0" '[ "$(counter)" = 0 ]'
check "fallback: scratch dropped" '[ ! -e "$D/readwrite/mcfg.tmp" ]'
out=$("$TOOL" confirm)
check "confirm without images is a no-op" 'grep -q "nothing to confirm" <<<"$out"'

# reset
mkdir -p "$D/original"; echo orig > "$D/original/modemst1.img"
echo img > "$D/modemst1.img"
"$TOOL" reset >/dev/null
check "reset: RESET marker" '[ -e "$D/RESET" ]'
check "reset: images gone" '[ ! -e "$D/modemst1.img" ]'
check "reset: originals kept" '[ -e "$D/original/modemst1.img" ]'
echo img > "$D/modemst1.img"   # a save that raced the reset
"$TOOL" prestart >/dev/null
check "prestart after reset: marker cleared" '[ ! -e "$D/RESET" ]'
check "prestart after reset: images gone" '[ ! -e "$D/modemst1.img" ]'
check "prestart after reset: counter 0" '[ "$(counter)" = 0 ]'
st=$("$TOOL" status)
check "status lists originals" 'grep -q "originals:      modemst1.img" <<<"$st"'

[ "$fail" = 0 ] && echo "fp6-rmtfs-shadow tests: all passed"
exit "$fail"
