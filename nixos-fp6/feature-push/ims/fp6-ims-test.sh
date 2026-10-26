#!/bin/sh
# FP6 VoLTE runtime test on the CURRENT (pre-module) image: transient units only.
# Run on the phone as root:  sudo sh fp6-ims-test.sh <cmd>
#   start         IMS PDN (fp6-ims-pdn-test) + imsd (imsd-test), runtime D-Bus
#                 policy (bind mount + ReloadConfig) and nft rules for rmnet_ims
#   status        registration state, calls, last imsd lines (numbers masked)
#   dial NUMBER   place a VoLTE call (only when the user has arranged the test)
#   accept|hangup [UNI]  answer / end a call (default: the first listed call)
#   dtmf UNI TONES
#   voiceprofile  switch the sound card to the Voice Call (earpiece) profile
#   hifi          back to HiFi
#   stop          stop imsd and the PDN, remove the nft rules, restore D-Bus
# On an image with services.fp6Ims use `systemctl start imsd` instead of start.
# Runtime only: no modem profile/NV/EFS/PDC write; everything is gone on reboot.
set -u
IMSD=${IMSD:-/nix/store/yqbk6kyn5d3q315bf5ngbjj26p6v0ia2-imsd-fp6-ofono-1.1.0}
PDN=${PDN:-/nix/store/6dg2cqyr72hj43c0c5fw410nh5izv2w6-fp6-ims-pdn}
DEV=rmnet_ims
B="busctl call net.catcrafts.IMS1 /net/catcrafts/IMS1 net.catcrafts.IMS1"
mask() { sed -E 's/[0-9a-f]{1,4}(:[0-9a-f]{0,4}){3,7}/<v6>/g; s/\+?[0-9]{7,}/<num>/g; s/(sip:|tel:)[^ >;"]*/\1<x>/g'; }
first_call() { $B GetCalls --json=short | python3 -c 'import json,sys; c=json.load(sys.stdin)["data"][0]; print(c[0]["uni"]["data"] if c else "")'; }
user_env() {
  U=$(loginctl list-sessions --no-legend | awk '$3!="" && $4=="seat0"{print $3; exit}')
  UID_=$(id -u "$U")
  echo "runuser -u $U -- env XDG_RUNTIME_DIR=/run/user/$UID_"
}

case "${1:-}" in
start)
  systemctl is-active -q fp6-ims-pdn-test || systemd-run --unit=fp6-ims-pdn-test \
    -p Type=notify -p NotifyAccess=main -p BindsTo=ofono.service -p After=ofono.service \
    -p ExecStopPost="-/run/current-system/sw/bin/ip link del $DEV" \
    "$PDN/bin/fp6-ims-pdn" --dev $DEV --mux 8 --apn ims || exit 1
  if ! grep -q "$IMSD/share/dbus-1/system.d" /etc/dbus-1/system.conf; then
    mkdir -p /run/fp6-ims
    sed "s#</busconfig>\$#<includedir>$IMSD/share/dbus-1/system.d</includedir></busconfig>#" \
      /etc/dbus-1/system.conf > /run/fp6-ims/system.conf
    mount --bind /run/fp6-ims/system.conf /etc/dbus-1/system.conf
    busctl call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig
  fi
  if ! nft list chain inet nixos-fw input-allow | grep -q fp6-ims-test; then
    nft insert rule inet nixos-fw input-allow iifname $DEV meta l4proto esp accept comment '"fp6-ims-test"'
    nft insert rule inet nixos-fw input-allow iifname $DEV udp dport '{ 45061, 45062, 50004 }' accept comment '"fp6-ims-test"'
    nft insert rule inet nixos-fw input-allow iifname $DEV tcp dport '{ 45061, 45062 }' accept comment '"fp6-ims-test"'
  fi
  systemctl is-active -q imsd-test || systemd-run --unit=imsd-test \
    -p BindsTo=fp6-ims-pdn-test.service -p After=fp6-ims-pdn-test.service \
    -p EnvironmentFile=/run/imsd.env -p StateDirectory=imsd -p StateDirectoryMode=0700 \
    -p KillMode=mixed -p TimeoutStopSec=10 \
    --setenv=IMSD_MEDIA="$IMSD/libexec/imsd-media" --setenv=DUMP_SIP=0 "$IMSD/bin/imsd"
  ;;
status)
  $B GetStatus --json=short 2>&1 | python3 -c 'import json,sys; d=json.load(sys.stdin)["data"][0]; print("registered", d["registered"]["data"], "expiry", d["expiry"]["data"])' 2>&1
  $B GetCalls --json=short 2>&1 | python3 -c 'import json,sys; [print(c["uni"]["data"], c["direction"]["data"], c["state"]["data"], c["reason"]["data"]) for c in json.load(sys.stdin)["data"][0]]' 2>&1
  journalctl -u imsd-test -u imsd -n 25 --no-pager -o cat | mask
  ;;
dial)    [ -n "${2:-}" ] || { echo "dial NUMBER"; exit 2; }; $B Dial s "$2" ;;
accept)  $B Accept s "${2:-$(first_call)}" ;;
hangup)  $B HangUp s "${2:-$(first_call)}" ;;
dtmf)    $B SendDtmf ss "$2" "$3" ;;
voiceprofile|hifi)
  E=$(user_env)
  prof=$([ "$1" = hifi ] && echo "HiFi (Mic1, Speaker)" || echo "${PROFILE:-Voice Call (Earpiece, Mic1)}")
  card=$($E pactl list cards short | awk '/platform-sound|sound/{print $2; exit}')
  echo "card $card profile -> $prof"
  $E pactl set-card-profile "$card" "$prof"
  ;;
stop)
  systemctl stop imsd-test fp6-ims-pdn-test 2>/dev/null
  for h in $(nft -a list chain inet nixos-fw input-allow | sed -n 's/.*fp6-ims-test.*# handle \([0-9]*\)/\1/p'); do
    nft delete rule inet nixos-fw input-allow handle "$h"
  done
  if mountpoint -q /etc/dbus-1/system.conf 2>/dev/null || grep -q " /etc/dbus-1/system.conf " /proc/self/mountinfo; then
    umount /etc/dbus-1/system.conf && busctl call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig
  fi
  ;;
*) sed -n '2,15p' "$0"; exit 2 ;;
esac
