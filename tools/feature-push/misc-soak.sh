#!/bin/sh
# READ-ONLY host-side soak logger (24 h battery / 7-day stability plan).
# Reuses tools/phone-health-log (no phone writes). Runs in the foreground;
# start it under the host job manager, e.g.:
#   systemd-run --user --collect -u fp6-misc-soak tools/feature-push/misc-soak.sh 24h
# Durations: 24h (battery, 5-min samples) or 7d (stability, 10-min samples).
. "$(dirname "$0")/misc-common.sh"
case "${1:-24h}" in
  24h) count=289; interval=300 ;;
  7d) count=1009; interval=600 ;;
  *) echo "usage: $0 24h|7d" >&2; exit 2 ;;
esac
exec python3 "$FP6_ROOT/tools/phone-health-log" --count "$count" --interval "$interval" \
  --output "$OUT_DIR/soak-${1:-24h}-$(stamp).jsonl"
