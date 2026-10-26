#!/bin/sh
# NetworkManager dispatcher hook for the Lomiri hotspot on the FP6.
# $1 = interface, $2 = action. @PATH@ is substituted by the NixOS module.
#
# 1. Uplink forwarding. NM 1.56 enables per-interface IPv4 forwarding on the
#    devices it configures while a shared connection is active. The cellular
#    uplink (oFono QRTR modem, IP on rmnet_data*) is configured outside NM's
#    IP path ("modem IP method unsupported"), so it keeps forwarding=0. The
#    replies to hotspot clients then arrive on rmnet_data* and are dropped
#    (no route for forwarded input). Mirror NM's "auto" behaviour for those
#    interfaces: 1 while a shared connection is active, else the kernel
#    default (net.ipv4.conf.default.forwarding).
# 2. Wi-Fi client after the hotspot. lomiri-indicator-network deactivates the
#    client connection before starting the AP. NM marks that connection
#    "blocked by user request", so it never autoconnects again when the
#    hotspot is turned off. When an AP-mode connection goes down, the indicator
#    has turned device autoconnect back on, and the Wi-Fi radio is enabled,
#    bring the most recently used client connection back up.
set -u
iface=${1:-}
action=${2:-}
PATH=@PATH@
export PATH
nmcli=nmcli

case "$action" in
  up|down|dhcp4-change|reapply) ;;
  *) exit 0 ;;
esac

sharing=0
for uuid in $($nmcli -g UUID,STATE connection show --active 2>/dev/null \
    | sed -n 's/^\([^:]*\):\(activated\|activating\)$/\1/p'); do
  if [ "$($nmcli -g ipv4.method connection show uuid "$uuid" 2>/dev/null)" = shared ]; then
    sharing=1
  fi
done
want=$sharing
[ "$sharing" = 1 ] || want=$(cat /proc/sys/net/ipv4/conf/default/forwarding)
for f in /proc/sys/net/ipv4/conf/rmnet_data*/forwarding; do
  [ -w "$f" ] || continue
  if [ "$(cat "$f")" != "$want" ]; then
    echo "$want" > "$f"
    echo "fp6-hotspot: $f=$want (sharing=$sharing)"
  fi
done

[ "$action" = down ] || exit 0
[ -n "${CONNECTION_UUID:-}" ] || exit 0
# The connection may already be gone (deleted); then this is not our case.
mode=$($nmcli -g 802-11-wireless.mode connection show uuid "$CONNECTION_UUID" 2>/dev/null) || exit 0
[ "$mode" = ap ] || exit 0

# The indicator re-enables device autoconnect right after the AP deactivation.
# If it stays off, the indicator still wants the hotspot (it reactivates it).
i=0
while [ $i -lt 10 ]; do
  [ "$($nmcli -g GENERAL.AUTOCONNECT device show "$iface" 2>/dev/null)" = yes ] && break
  sleep 0.5
  i=$((i + 1))
done
[ $i -lt 10 ] || exit 0
[ "$($nmcli radio wifi 2>/dev/null)" = enabled ] || exit 0
case "$($nmcli -g GENERAL.STATE device show "$iface" 2>/dev/null)" in
  30*) ;;   # disconnected
  *) exit 0 ;;
esac

# Newest autoconnect Wi-Fi client connection first; try at most three.
tried=0
$nmcli -t -f UUID,TYPE,AUTOCONNECT,TIMESTAMP connection show 2>/dev/null \
  | awk -F: '$2 == "802-11-wireless" && $3 == "yes" { print $4, $1 }' \
  | sort -rn | while read -r _ uuid; do
    [ $tried -lt 3 ] || break
    [ "$($nmcli -g 802-11-wireless.mode connection show uuid "$uuid" 2>/dev/null)" = ap ] && continue
    tried=$((tried + 1))
    if $nmcli --wait 20 connection up uuid "$uuid" ifname "$iface" >/dev/null 2>&1; then
      echo "fp6-hotspot: restored Wi-Fi client $uuid on $iface"
      break
    fi
  done
exit 0
