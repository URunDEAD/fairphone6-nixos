# Sourced by misc-*.sh. Read-only helpers for the FP6 misc feature push.
set -eu
FP6_ROOT=$(cd "$(dirname "$0")/../.." && pwd)
PHONE="$FP6_ROOT/tools/phone-ssh"
SUDO='SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p ""'
OUT_DIR=${OUT_DIR:-$FP6_ROOT/results/feature-push-20261024/misc}
mkdir -p "$OUT_DIR"
ph() { timeout "${PH_TIMEOUT:-60}" "$PHONE" "$@"; }
stamp() { date -u +%Y%m%dT%H%M%SZ; }
