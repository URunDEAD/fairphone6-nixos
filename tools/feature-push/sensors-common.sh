# Sourced by tools/feature-push/sensors-*.sh. Live tests for the FP6 sensors
# feature push. Each script says whether it is READ-ONLY or changes state.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
PHONE="$HERE/../phone-ssh"
ROOT='SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p ""'
ALS=/sys/bus/iio/devices/iio:device1      # verified name stk36c61 below
TOUCH=/sys/bus/spi/devices/spi0.0          # NEVER read $TOUCH/reset_device: reading it resets the controller
phone() { "$PHONE" "$@"; }
prompt() { printf '\n>>> %s  [press Enter]\n' "$*"; read -r _; }
require_als() {
  n=$(phone "cat $ALS/name")
  [ "$n" = stk36c61 ] || { echo "iio:device1 is '$n', not stk36c61; adjust ALS=" >&2; exit 1; }
}
