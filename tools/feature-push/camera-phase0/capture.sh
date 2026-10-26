#!/usr/bin/env bash
# Host side: run probe.sh on the phone (as root, for the dma-heap), pull the
# last frame, analyse it. usage: capture.sh NAME DEVICE WxH SECONDS [ENV=VAL...]
# OUT dir: $PHASE0_OUT (default results/feature-push-20261024/camera/phase0/captures)
set -eu
here=$(cd "$(dirname "$0")" && pwd); root=$here/../../..
name=$1 dev=$2 size=$3 secs=$4; shift 4
out=${PHASE0_OUT:-$root/results/feature-push-20261024/camera/phase0/captures}
py=${PHASE0_PY:-python3}
mkdir -p "$out"
# The camera is shared (Phase 1 probes, the user's app): wait up to 5 min for it.
for i in $(seq 1 60); do
  "$root/tools/phone-ssh" "pgrep -f '[l]omiri-camera-app|[g]st-launch-1.0|/bin/[c]am( |\$)|[q]cam|[f]p6camera-selftest' >/dev/null" || break
  [ "$i" = 60 ] && { echo "camera still busy" >&2; exit 3; }
  sleep 5
done
envs=""; for a in "$@"; do envs="$envs $(printf %q "$a")"; done
# Phone sudo password: $FP6_PHONE_PASSWORD, or the untracked file
# $root/.fp6-phone-secret (ignored by git). Fed to sudo on stdin only.
pw=${FP6_PHONE_PASSWORD:-}
[ -n "$pw" ] || { [ -r "$root/.fp6-phone-secret" ] && pw=$(cat "$root/.fp6-phone-secret"); }
[ -n "$pw" ] || { echo "set FP6_PHONE_PASSWORD or create .fp6-phone-secret" >&2; exit 2; }
printf '%s\n' "$pw" | "$root/tools/phone-ssh" "sudo -S -p '' env $envs ~/phase0/probe.sh '$dev' $secs /tmp/p0/$name $size" >&2
last=$("$root/tools/phone-ssh" "cat /tmp/p0/$name/last.txt")
"$root/tools/phone-ssh" "cat $last" > "$out/$name.rgb"
"$root/tools/phone-ssh" "grep -E 'agc.cpp|awb.cpp:4|af.cpp' /tmp/p0/$name/lc.log | tail -400" > "$out/$name-3a.log" || true
"$py" "$here/analyze.py" "$out/$name.rgb" "${size%x*}" "${size#*x}" "$out/$name" "$name"
rm -f "$out/$name.rgb"
