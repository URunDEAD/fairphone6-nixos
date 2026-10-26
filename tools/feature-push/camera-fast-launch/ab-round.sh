#!/usr/bin/env bash
# A/B rounds of measure.sh (phone side), detached from the ssh session:
#   setsid -f ab-round.sh OUTDIR A_BIN B_BIN ROUNDS  > OUTDIR.out
# Each round: A (cold warm warm), B (cold warm warm), each under its own
# GUI lock hold. B's very first run (empty QML cache) is round 0 "first".
set -u
out=$1 a=$2 b=$3 rounds=${4:-2}
here=$(dirname "$0")
mkdir -p "$out"
FL_ENV= "$here/measure.sh" "$out/b-first" "$b" cold
for r in $(seq 1 "$rounds"); do
  "$here/measure.sh" "$out/a-r$r" "$a" cold warm warm
  "$here/measure.sh" "$out/b-r$r" "$b" cold warm warm
done
FL_ENV="QML_DISABLE_DISK_CACHE=1 FP6_CAMERA_AE_SEED=0" "$here/measure.sh" "$out/b-nocache" "$b" cold warm
echo "AB DONE"
