#!/bin/sh
# READ-ONLY (default): runs 'fp6-factory-reset plan' (sizes + mount-point
# safety check; deletes nothing). Requires fp6FactoryReset in the booted
# generation.
# --request: !!! DESTRUCTIVE ON NEXT BOOT, USER DECISION ONLY !!! writes the
#   request file; the reset (quarantine mode = move aside, reversible by
#   moving dirs back) happens on the following boot. Cancel before reboot
#   with --cancel. Never run on the production phone without the user.
. "$(dirname "$0")/misc-common.sh"
case "${1:-}" in
  --request) ph "$SUDO fp6-factory-reset request --yes-erase-user-data" ;;
  --cancel) ph "$SUDO fp6-factory-reset cancel" ;;
  *) ph "command -v fp6-factory-reset >/dev/null && $SUDO fp6-factory-reset plan || echo 'fp6-factory-reset not installed in this generation'" ;;
esac
