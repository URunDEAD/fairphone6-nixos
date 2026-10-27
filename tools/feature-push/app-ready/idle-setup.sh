#!/usr/bin/env bash
# round.sh SETUP wrapper for battery.sh: optional KEEP list, then the idle run.
here=$(dirname "$(readlink -f "$0")")
if [ -n "${KEEP:-}" ]; then
  "$here/ar-test.sh" gs set com.fairphone6.app-ready keep-ready "$KEEP"
  sleep 2
fi
OUT=$here/out/idle.txt "$here/battery.sh"
