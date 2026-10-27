#!/usr/bin/env bash
# Undo after-setup.sh: every com.fairphone6.app-ready key back to its default
# (lists empty). zram and the service stay as the system configures them.
here=$(dirname "$(readlink -f "$0")")
"$here/ar-test.sh" reset >/dev/null
"$here/ar-test.sh" gs get com.fairphone6.app-ready keep-ready
