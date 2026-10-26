#!/bin/sh
# READ-ONLY. Lux/raw response to cover / room / torch-or-window light.
. "$(dirname "$0")/sensors-common.sh"
T=${1:-5}
require_als
phone "echo it=\$(cat $ALS/in_illuminance_integration_time) scale=\$(cat $ALS/in_illuminance_scale)"
sample() {
  phone "i=0; while [ \$i -lt $((T*2)) ]; do echo \$(cat $ALS/in_illuminance_raw $ALS/in_intensity_clear_raw $ALS/in_intensity_red_raw $ALS/in_intensity_green_raw $ALS/in_intensity_blue_raw 2>&1); sleep 0.5; i=\$((i+1)); done" |
    awk -v tag="$1" 'NF==5{n++; a+=$1; c+=$2; r+=$3; g+=$4; b+=$5; next}{err++}
      END{printf "%-8s n=%d als=%.1f clear=%.1f r=%.1f g=%.1f b=%.1f errors=%d\n", tag, n, a/(n?n:1), c/(n?n:1), r/(n?n:1), g/(n?n:1), b/(n?n:1), err}'
}
prompt "Phone face-up in normal room light"; sample room
prompt "Cover the top bezel completely with a finger"; sample dark
prompt "Point the top of the screen at a window / lamp / another phone's torch"; sample bright
echo
echo "PASS criteria: dark < room < bright on als AND clear; if als stays 0 while clear changes,"
echo "the STK36C61 ALS channel/gain table is wrong (see results/feature-push-20261024/sensors/README.md)."
phone "timeout $T monitor-sensor --light 2>&1 | tail -4" || true
