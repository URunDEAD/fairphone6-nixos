#!/usr/bin/env bash
# Real-USB test of fp6-usb-drived on the phone WITHOUT activating a system:
# the helper runs from its store path as a transient systemd unit with the
# module's sandbox, on a private D-Bus daemon (/run/fp6ud-test/bus; no
# polkit there, so --no-polkit; the uid check and all path checks stay on).
# The laptop on the same USB cable sees the drive.
#
#   test/phone-usb-test.sh setup CORE_STORE_PATH
#   test/phone-usb-test.sh as-user  'fp6-usb-drive-cli ...'   (uid 10000 client)
#   test/phone-usb-test.sh gadget                             (links, UDC, state)
#   test/phone-usb-test.sh teardown                           (stop = restore, remove units)
#
# Env: FP6_WIFI=192.168.x.y (fallback ssh; checked before setup).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
ws=$(cd "$here/../../../.." && pwd)
phone=$ws/tools/phone-ssh
SUDO='SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p ""'
cmd=${1:?setup|as-user|gadget|teardown}
wifi() { FP6_SSH_USB=0 FP6_SSH_ADDRESS=${FP6_WIFI:?set FP6_WIFI} timeout 20 "$phone" "$@"; }

case $cmd in
setup)
  core=${2:?core store path}
  wifi 'echo wifi-fallback-ok' | grep -q wifi-fallback-ok || { echo "no Wi-Fi fallback; refusing" >&2; exit 1; }
  "$phone" "set -e; $SUDO mkdir -p -m 0755 /run/fp6ud-test
    printf '%s\n' '<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">' \
      '<busconfig><type>session</type><listen>unix:path=/run/fp6ud-test/bus</listen><auth>EXTERNAL</auth>' \
      '<policy context=\"default\"><allow user=\"*\"/><allow send_destination=\"*\"/><allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>' \
      | $SUDO tee /run/fp6ud-test/bus.conf >/dev/null
    $SUDO systemd-run --unit=fp6ud-test-bus --collect -p Type=simple \
      \$(readlink -f \$(command -v dbus-daemon)) --config-file=/run/fp6ud-test/bus.conf --nofork --nopidfile
    sleep 1
    $SUDO systemd-run --unit=fp6ud-test-helper --collect -p Type=simple \
      -E DBUS_SESSION_BUS_ADDRESS=unix:path=/run/fp6ud-test/bus \
      -p CapabilityBoundingSet='CAP_DAC_OVERRIDE CAP_DAC_READ_SEARCH CAP_SYS_PTRACE' \
      -p NoNewPrivileges=yes -p ProtectSystem=strict -p ProtectHome=read-only \
      -p ReadWritePaths='/sys/kernel/config /run -/home/user/Images' -p PrivateTmp=yes -p PrivateNetwork=yes \
      -p RestrictAddressFamilies=AF_UNIX -p RestrictNamespaces=yes -p LockPersonality=yes \
      -p MemoryDenyWriteExecute=yes -p ProtectKernelLogs=yes -p ProtectControlGroups=yes \
      -p SystemCallArchitectures=native -p SystemCallFilter=@system-service -p UMask=0077 \
      -p TimeoutStopSec=45 \
      -p ExecStopPost='$core/bin/fp6-usb-drived --recover --uid 10000 --allow /home/user/Images' \
      $core/bin/fp6-usb-drived --session-bus --no-polkit --uid 10000 --allow /home/user/Images \
        --exclusive-max-minutes \${FP6UD_EXCL_MIN:-3}
    sleep 1; systemctl is-active fp6ud-test-helper"
  ;;
as-user)
  "$phone" "export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/fp6ud-test/bus FP6_USB_DRIVE_SESSION_BUS=1 PATH=\$PATH:${CORE:?set CORE}/bin; $2"
  ;;
gadget)
  "$phone" 'g=/sys/kernel/config/usb_gadget/g1; echo "UDC=$(cat $g/UDC) state=$(cat /sys/class/udc/*/state)"; ls -l $g/configs/c.1 | grep -- "->" | sed "s/.* \([^ ]*\) -> .*functions\//\1 -> /"; ls $g/functions; ls /run/fp6-usb-drive 2>/dev/null; ip -4 -br addr show usb0'
  ;;
teardown)
  "$phone" "$SUDO systemctl stop fp6ud-test-helper 2>/dev/null; $SUDO systemctl stop fp6ud-test-bus 2>/dev/null; $SUDO rm -rf /run/fp6ud-test; systemctl is-active fp6ud-test-helper fp6ud-test-bus || true"
  ;;
*) echo "unknown command" >&2; exit 2 ;;
esac
