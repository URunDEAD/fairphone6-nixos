#!/usr/bin/env bash
# Third round (phone side, detached): before (A) / after (C) launch times,
# alternating, then background parking and resume with C.
#   round3.sh OUTDIR A_BIN C_BIN ROUNDS
set -u
out=$1 a=$2 c=$3 rounds=${4:-3}
here=$(dirname "$0")
mkdir -p "$out"
for r in $(seq 1 "$rounds"); do
  "$here/measure.sh" "$out/a-r$r" "$a" cold warm warm
  "$here/measure.sh" "$out/c-r$r" "$c" cold warm warm
done
FL_WAIT=8 "$here/measure.sh" "$out/c-bg-default" "$c" warm resume resume
FL_WAIT=8 FL_ENV="FP6_CAMERA_KEEP_READY=1" "$here/measure.sh" "$out/c-bg-keepready" "$c" warm resume resume
FL_WAIT=8 "$here/measure.sh" "$out/a-bg" "$a" warm resume
echo "R3 DONE"
