#!/usr/bin/env bash
# detach.sh NAME [round.sh args...]: run round.sh as a transient user unit so
# a dropped SSH/USB link does not kill it mid-round. Login digits on stdin are
# kept in a 0600 file under $XDG_RUNTIME_DIR only until round.sh has read them.
# Log: ~/fp6-app-ready-test/out/NAME.log; env SETUP/TEARDOWN/BUDGET/APPREADY/OTHER pass through.
set -u
here=$(dirname "$(readlink -f "$0")")
export XDG_RUNTIME_DIR=/run/user/$(id -u)
name=$1; shift
pinfile=$XDG_RUNTIME_DIR/fp6-ar-pin
( umask 077; cat > "$pinfile" )
mkdir -p "$here/out"
envs=()
for v in SETUP TEARDOWN BUDGET APPREADY OTHER LOMIRI_ENV RESUME_WAIT KEEP APPS SECONDS_IDLE; do [ -n "${!v:-}" ] && envs+=("-E" "$v=${!v}"); done
args=$(printf '%q ' "$@")
systemd-run --user --unit="fp6-ar-$name" --collect "${envs[@]}" \
  bash -c "\"$here/round.sh\" $args < \"$pinfile\" > \"$here/out/$name.log\" 2>&1 & sleep 2; rm -f \"$pinfile\"; wait"
