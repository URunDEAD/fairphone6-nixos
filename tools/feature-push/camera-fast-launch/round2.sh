#!/usr/bin/env bash
# Second test round (phone side, detached): AE seed, background parking,
# resume from the background, memory.  round2.sh OUTDIR B_BIN
set -u
out=$1 b=$2
here=$(dirname "$0")
mkdir -p "$out"
sd=""
for d in /sys/class/video4linux/v4l-subdev*; do
  case "$(cat $d/name)" in imx896*) sd=/dev/$(basename $d) ;; esac
done
# the IMX896 back at its driver defaults = the first use after a boot
reset="v4l2-ctl -d $sd -c exposure=3743,analogue_gain=0"
# AE seed on / off, alternating (warm page cache)
for r in 1 2; do
  FL_PRE="$reset" "$here/measure.sh" "$out/seed-on-r$r" "$b" warm
  FL_PRE="$reset" FL_ENV="FP6_CAMERA_AE_SEED=0" "$here/measure.sh" "$out/seed-off-r$r" "$b" warm
done
# background: default (release after 2 s) and Keep ready (opened, not streaming)
FL_WAIT=8 "$here/measure.sh" "$out/bg-default" "$b" warm resume resume
FL_WAIT=8 FL_ENV="FP6_CAMERA_KEEP_READY=1" "$here/measure.sh" "$out/bg-keepready" "$b" warm resume resume
echo "R2 DONE"
