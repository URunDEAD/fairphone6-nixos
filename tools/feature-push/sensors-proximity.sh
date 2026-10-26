#!/bin/sh
# READ-ONLY (sysfs reads; optional D-Bus ClaimProximity through monitor-sensor).
# Usage: sensors-proximity.sh [seconds-per-phase]
. "$(dirname "$0")/sensors-common.sh"
T=${1:-6}
require_als
echo "nearlevel=$(phone "cat $ALS/in_proximity_nearlevel") scale=$(phone "cat $ALS/in_proximity_scale")"
sample() {
  phone "i=0; while [ \$i -lt $((T*2)) ]; do cat $ALS/in_proximity_raw 2>&1; sleep 0.5; i=\$((i+1)); done" |
    awk -v tag="$1" '/^[0-9]+$/{n++; s+=$1; if(min==""||$1<min)min=$1; if($1>max)max=$1; next} {err++}
      END{printf "%-10s samples=%d min=%s max=%s mean=%.1f read-errors=%d\n", tag, n, min, max, (n?s/n:0), err}'
}
prompt "Keep the top of the phone UNCOVERED"; sample far
prompt "COVER the top bezel (earpiece/sensor) with your hand ~1 cm away"; sample near
prompt "Uncover again"; sample far-again
echo
echo "PASS criteria: near.min > nearlevel(+10%) and far.max < nearlevel; read-errors ~0."
echo "iio-sensor-proxy view (claims proximity for $T s):"
phone "timeout $T monitor-sensor --proximity 2>&1 | tail -5" || true
phone "$ROOT journalctl -k -b --no-pager | grep -c 'NAK bailout'" | sed 's/^/i2c-7 NAK count this boot: /'
