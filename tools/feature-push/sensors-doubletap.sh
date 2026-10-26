#!/bin/sh
# CHANGES STATE (writes ESWIN gesture_wakeup/lp_mode, may suspend). Run only on the
# integrated kernel (patch 0003) with recovery access. Usage:
#   sensors-doubletap.sh blank     # awake + panel blanked (Lomiri path, no suspend)
#   sensors-doubletap.sh suspend   # system suspend (s2idle/deep) wake by double tap
. "$(dirname "$0")/sensors-common.sh"
MODE=${1:-blank}
phone "test -e $TOUCH/lp_mode" || { echo "lp_mode missing: kernel lacks feature-push patch 0003" >&2; exit 1; }
phone "cat $TOUCH/gesture_wakeup $TOUCH/lp_mode $TOUCH/power/wakeup"
# Prefer the D-Bus path that Lomiri System Settings uses, when the backend runs.
if phone 'busctl --system status com.canonical.Unity.Screen >/dev/null 2>&1'; then
  phone 'busctl --system call com.canonical.Unity.Screen /com/canonical/Unity/Screen com.canonical.Unity.Screen getDoubleTapToWakeSupported'
  phone 'busctl --system call com.canonical.Unity.Screen /com/canonical/Unity/Screen com.canonical.Unity.Screen setDoubleTapToWakeEnabled b true'
else
  phone "$ROOT sh -c 'echo 2 > $TOUCH/gesture_wakeup'"
fi
phone "cat $TOUCH/gesture_wakeup"
phone "$ROOT dmesg -w 2>/dev/null | grep --line-buffered -E 'gesture lp|eph_suspend|eph_resume|PM: suspend|Failed to enter' " &
DM=$!
trap 'kill $DM 2>/dev/null || true' EXIT
case $MODE in
blank)
  if phone 'busctl --system call com.canonical.Unity.Screen /com/canonical/Unity/Screen com.canonical.Unity.Screen setScreenPowerMode si off 2 2>/dev/null'; then :
  else phone "$ROOT sh -c 'echo 1 > $TOUCH/lp_mode'"; fi
  phone "echo lp_mode=\$(cat $TOUCH/lp_mode)"
  prompt "Screen should be dark. Single-swipe first (must NOT wake / no touches), then DOUBLE-TAP the screen"
  phone 'timeout 10 python3 -c "
import os,struct,glob
n=[e for e in glob.glob(\"/sys/class/input/event*\") if open(e+\"/device/name\").read().strip()==\"ESWIN EPH86XX Touchscreen\"][0]
fd=os.open(\"/dev/input/\"+os.path.basename(n),os.O_RDONLY);S=struct.Struct(\"llHHi\")
while 1:
  d=os.read(fd,S.size*16)
  for o in range(0,len(d),S.size):
    t,c,v=S.unpack_from(d,o)[2:]
    if t==1 and c==143: print(\"KEY_WAKEUP\",v,flush=True)
"' || true
  phone "echo lp_mode=\$(cat $TOUCH/lp_mode)  backlight=\$(cat /sys/class/backlight/*/brightness)"
  echo "PASS: KEY_WAKEUP 1/0 seen; with fp6-lomiri-powerd --screen-power the backlight returns and lp_mode=0."
  phone "$ROOT sh -c 'echo 0 > $TOUCH/lp_mode'" || true
  ;;
suspend)
  prompt "System will suspend now. Wait 10 s, then DOUBLE-TAP the dark screen"
  phone "$ROOT systemctl suspend" || true
  sleep 25
  phone "$ROOT journalctl -k -b --no-pager | grep -E 'gesture lp|PM: suspend|eph_resume|Wakeup pending' | tail -12"
  phone "cat /sys/power/wakeup_count; grep -E 'spi0.0|eswin' /sys/kernel/debug/wakeup_sources 2>/dev/null || true"
  echo "PASS: 'gesture lp entered' before suspend entry, resume shortly after the tap, touch works after."
  ;;
esac
echo "Then verify normal touch works (tap/scroll) — reset-based LP exit must restore reports."
