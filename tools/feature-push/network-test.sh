#!/usr/bin/env bash
# FP6 network feature-push live test (2026-10-24). Run from the workspace root
# (<workspace>) by the parent agent / user, NOT unattended.
#
#   tools/feature-push/network-test.sh preflight          read-only status
#   tools/feature-push/network-test.sh wifi-resume        read-only: Wi-Fi reconnect delay after each suspend this boot
#   tools/feature-push/network-test.sh bt-scan [secs]     BR/EDR+LE discovery, lists nearby devices
#   tools/feature-push/network-test.sh bt-pair MAC        pair+trust+connect a headset/speaker (put it in pairing mode first)
#   tools/feature-push/network-test.sh bt-audio MAC       play a test sound to the headset's PipeWire sink, show profiles
#   tools/feature-push/network-test.sh bt-forget MAC      disconnect+remove the test pairing
#   tools/feature-push/network-test.sh hotspot-concurrent [secs]   STA+AP probe on ap0 (hostapd), keeps wlan0 connected
#   tools/feature-push/network-test.sh hotspot-nm [secs]  NetworkManager shared hotspot on wlan0 (= Lomiri behaviour; drops client Wi-Fi)
#   tools/feature-push/network-test.sh flight [secs]      flight mode on, verify, restore
#   FP6_NFC_RF_OK=1 tools/feature-push/network-test.sh nfc-poll [secs]   power NFC RF + poll for a tag via neard, then power off
#
# Safety model
# * Every state-changing phase requires the USB management link (172.16.42.1,
#   usb0 is unmanaged by NetworkManager and is never touched) and refuses to
#   run over Wi-Fi SSH.
# * Phases that touch radios first arm a systemd-run watchdog on the phone that
#   restores the saved state after the hold time + 90 s even if this SSH
#   session dies; normal teardown cancels it.
# * hotspot-concurrent never changes wlan0: it adds a separate AP vdev ap0
#   (NM-unmanaged via a volatile /run udev rule) on the SAME channel as the
#   station (ath11k WCN6750: #{managed}<=1, #{AP}<=16, #channels<=1).
# * hotspot-nm reproduces what Lomiri's hotspot does: NetworkManager runs the
#   AP on wlan0, so the client Wi-Fi is disconnected for the hold time and is
#   re-activated afterwards. (NM cannot use ap0 concurrently: wpa_supplicant
#   forces every interface it manages to station mode, which the single-STA
#   combination rejects.)
# * Fixed WPA2 test network: SSID FP6-TEST-AP, PSK fp6-feature-push-test
#   (test-only credential; the profile is deleted on teardown).
# * No monitor mode, no sniffing, no attacks; NFC RF only with FP6_NFC_RF_OK=1.
#
# Optional env:
#   FP6_NET_TOOLS=/nix/store/...-fp6-network-test-tools  (hostapd, pactl, iw on the phone;
#     build: nixos-fp6/feature-push/network/build.nix testTools, copy closure to the phone)
set -euo pipefail
cd "$(dirname "$0")/../.."

SSID=FP6-TEST-AP
PSK=fp6-feature-push-test
ASKPASS='SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p ""'
TOOLS=${FP6_NET_TOOLS:-}

die() { echo "network-test: $*" >&2; exit 1; }

# Phone-side helper library, sent on stdin to a root bash on the phone.
read -r -d '' PHONE_LIB <<'PHONE' || true
set -euo pipefail
export PATH="${FP6_NET_TOOLS:+$FP6_NET_TOOLS/bin:}/run/current-system/sw/bin:/run/wrappers/bin"
RUN=/run/fp6-net-test
mkdir -p -m 0700 "$RUN"
say() { printf '[phone] %s\n' "$*"; }
sta_conn() { nmcli -g GENERAL.CONNECTION device show wlan0 2>/dev/null || true; }
sta_freq() { iw dev wlan0 link 2>/dev/null | sed -nE 's/^\s*freq: ([0-9]+).*/\1/p' | cut -d. -f1; }
gw4() { ip -4 route show default dev wlan0 2>/dev/null | awk '{print $3; exit}'; }
wifi_ok() {
  local gw; gw=$(gw4)
  [ -n "$gw" ] && ping -I wlan0 -c 3 -W 2 "$gw" >/dev/null 2>&1
}
wait_wifi() {
  local i
  for i in $(seq 1 "${1:-60}"); do wifi_ok && { say "wlan0 back after ${i}s ($(sta_conn))"; return 0; }; sleep 1; done
  say "WARNING: wlan0 not reconnected after ${1:-60}s"; return 1
}
arm_watchdog() { # $1 seconds, restore script at $RUN/restore.sh
  systemctl stop fp6-nettest-watchdog.timer fp6-nettest-watchdog.service >/dev/null 2>&1 || true
  systemd-run --quiet --unit=fp6-nettest-watchdog --on-active="$1" /bin/sh "$RUN/restore.sh"
  say "watchdog armed: restore in $1 s unless teardown cancels it"
}
disarm_watchdog() { systemctl stop fp6-nettest-watchdog.timer >/dev/null 2>&1 || true; }
radio_status() {
  rfkill list
  nmcli -t radio all
  nmcli -t -f DEVICE,TYPE,STATE,CONNECTION device | grep -v -E '^(lo|veth|waydroid)'
  timeout 5 bluetoothctl show 2>/dev/null | grep -E 'Powered|PowerState|Discovering' || true
  if busctl --system status org.freedesktop.URfkill >/dev/null 2>&1; then
    say "URfkill provider: present, IsFlightMode=$(busctl --system call org.freedesktop.URfkill /org/freedesktop/URfkill org.freedesktop.URfkill IsFlightMode | awk '{print $2}')"
  else
    say "URfkill provider: absent (Lomiri flight-mode switch is inert without network-fp6.nix)"
  fi
  if busctl --system status org.freedesktop.ModemManager1 >/dev/null 2>&1; then mmcli -L 2>/dev/null || true; fi
  if busctl --system status org.ofono >/dev/null 2>&1; then
    busctl --system call org.ofono / org.ofono.Manager GetModems 2>/dev/null | head -c 400; echo
  fi
}
PHONE

phone() { # phone <usb-required 0|1> <phase script...>
  local usb=$1; shift
  local env="FP6_NET_TOOLS=$TOOLS"
  if [ "$usb" = 1 ]; then export FP6_SSH_USB=1; else export FP6_SSH_USB=${FP6_SSH_USB:-auto}; fi
  { printf '%s\n' "$PHONE_LIB"; printf '%s\n' "$@"; } |
    tools/phone-ssh "$ASKPASS env $env bash -s"
}

need_usb() {
  FP6_SSH_USB=1 tools/phone-ssh true 2>/dev/null ||
    die "USB management link 172.16.42.1 not verified; refusing a radio-changing phase over Wi-Fi SSH"
}

mac_arg() { [[ ${1:-} =~ ^([0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}$ ]] || die "need a Bluetooth address AA:BB:CC:DD:EE:FF"; }

cmd=${1:-}; shift || true
case "$cmd" in
preflight)
  phone 0 '
say "kernel $(uname -r)"; radio_status
iw phy phy0 info | sed -n "/Supported interface modes/,/Band 1/p;/valid interface combinations/,/HT Capability/p" | grep -v -E "Band 1|HT Capability"
iw dev wlan0 info | grep -E "type|channel"
iw reg get | sed -n "/phy#0/,+1p"
nft list tables
nft list chain inet nixos-fw input-allow 2>/dev/null | grep -E "wlan0|ap0" || say "no wlan0 firewall allowances (hotspot DHCP blocked until network-fp6.nix is deployed)"
ls /sys/class/nfc; cat /sys/class/nfc/nfc0/device/modalias 2>/dev/null || true
busctl --system status org.neard >/dev/null 2>&1 && say "neard running" || say "neard not running"
[ -n "${FP6_NET_TOOLS:-}" ] && ls "$FP6_NET_TOOLS/bin" | tr "\n" " "; echo'
  ;;

wifi-resume)
  phone 0 '
journalctl -b -o short-unix --no-pager -t kernel -t wpa_supplicant 2>/dev/null |
  awk "/PM: suspend exit/ {t=\$1; pending=1} pending && /wlan0: CTRL-EVENT-CONNECTED/ {printf \"resume->connected %.1f s\n\", \$1-t; pending=0}
       /CTRL-EVENT-BEACON-LOSS/ {print \"beacon loss at\", strftime(\"%T\", \$1)}" | tail -20
say "current: $(sta_conn), $(iw dev wlan0 link | grep -E "signal|freq" | tr -s " " | tr "\n" " ")"'
  ;;

bt-scan)
  secs=${1:-15}
  phone 0 "
bluetoothctl --timeout $secs scan on >/dev/null 2>&1 || true
say 'devices seen (address name):'; bluetoothctl devices | sed 's/^Device //'"
  ;;

bt-pair)
  mac_arg "${1:-}"; need_usb
  phone 1 "
bluetoothctl pairable on >/dev/null
bluetoothctl --timeout 15 scan on >/dev/null 2>&1 || true
bluetoothctl --agent NoInputNoOutput --timeout 40 pair $1 || say 'pair returned non-zero (already paired, or the device needs a PIN: use Lomiri Settings > Bluetooth)'
bluetoothctl trust $1
bluetoothctl --timeout 20 connect $1 || true
sleep 3
bluetoothctl info $1 | grep -E 'Name|Paired|Bonded|Trusted|Connected|UUID: (Audio|Hands|Headset|A/V)'
bluetoothctl pairable off >/dev/null"
  ;;

bt-audio)
  mac_arg "${1:-}"
  phone 0 "
u=\$(id -u user); export XDG_RUNTIME_DIR=/run/user/\$u
node=bluez_output.\$(echo $1 | tr : _)
runuser -u user -- wpctl status | sed -n '/Audio/,/Video/p' | grep -i -E 'bluez|sink|$1' || true
snd=\$(ls /run/current-system/sw/share/sounds/*/*/*.ogg /run/current-system/sw/share/sounds/*/*.ogg 2>/dev/null | head -1)
[ -n \"\$snd\" ] || { say 'no system sound file found'; exit 0; }
say \"playing \$snd to \$node.*\"
target=\$(runuser -u user -- pw-cli ls Node 2>/dev/null | sed -nE 's/.*node.name = \"(bluez_output[^\"]*)\".*/\1/p' | head -1)
[ -n \"\$target\" ] || { say 'no bluez_output node: is the headset connected with A2DP?'; exit 1; }
runuser -u user -- pw-play --target \"\$target\" \"\$snd\"
if command -v pactl >/dev/null; then
  card=bluez_card.\$(echo $1 | tr : _)
  runuser -u user -- pactl list cards | sed -n \"/\$card/,/^Card/p\" | grep -E 'Active Profile|a2dp|headset-head-unit' | head
  say 'HFP check: pactl set-card-profile '\$card' headset-head-unit, then record with pw-record --target bluez_input...'
fi"
  ;;

bt-forget)
  mac_arg "${1:-}"
  phone 0 "bluetoothctl disconnect $1 || true; bluetoothctl remove $1"
  ;;

hotspot-concurrent)
  secs=${1:-60}; need_usb
  [ -n "$TOOLS" ] || die "set FP6_NET_TOOLS to the fp6-network-test-tools store path on the phone (hostapd)"
  phone 1 "
command -v hostapd >/dev/null || { say 'hostapd missing in FP6_NET_TOOLS'; exit 1; }
wifi_ok || { say 'wlan0 is not connected; this probe needs an active station'; exit 1; }
freq=\$(sta_freq); conn=\$(sta_conn)
say \"station: \$conn at \$freq MHz\"
if [ \"\$freq\" -lt 3000 ]; then hw=g; ch=\$(( (freq - 2407) / 5 ));
elif [ \"\$freq\" -lt 5900 ]; then hw=a; ch=\$(( (freq - 5000) / 5 ));
else say '6 GHz station channel: AP probe skipped'; exit 0; fi
if iw phy phy0 channels 2>/dev/null | grep -A3 \" \$freq MHz\" | grep -q -i radar; then say 'DFS station channel: AP probe skipped'; exit 0; fi
cat > \$RUN/restore.sh <<EOF
pkill -F \$RUN/hostapd.pid 2>/dev/null || true
iw dev ap0 del 2>/dev/null || true
rm -f /run/udev/rules.d/90-fp6-nettest-ap0.rules; udevadm control --reload
EOF
arm_watchdog \$(( $secs + 90 ))
mkdir -p /run/udev/rules.d
echo 'ACTION==\"add\", SUBSYSTEM==\"net\", KERNEL==\"ap0\", ENV{NM_UNMANAGED}=\"1\"' > /run/udev/rules.d/90-fp6-nettest-ap0.rules
udevadm control --reload
iw dev wlan0 interface add ap0 type __ap
mac=\$(cat /sys/class/net/wlan0/address); first=\$(( 0x\${mac%%:*} | 0x02 ))
ip link set ap0 address \$(printf '%02x' \$first):\${mac#*:}
sleep 2; nmcli -t -f DEVICE,STATE device | grep '^ap0' || true
cat > \$RUN/hostapd.conf <<EOF
interface=ap0
driver=nl80211
ssid=$SSID
hw_mode=\$hw
channel=\$ch
ieee80211n=1
wmm_enabled=1
wpa=2
wpa_key_mgmt=WPA-PSK
rsn_pairwise=CCMP
wpa_passphrase=$PSK
ctrl_interface=\$RUN/hostapd
EOF
chmod 600 \$RUN/hostapd.conf
if hostapd -B -P \$RUN/hostapd.pid \$RUN/hostapd.conf >\$RUN/hostapd.log 2>&1; then
  sleep 4
  iw dev ap0 info | grep -E 'type|ssid|channel'
  say 'AP up. Connect a client to $SSID (static IP; no DHCP in this probe) during the hold.'
else
  say 'hostapd failed:'; tail -20 \$RUN/hostapd.log
fi
wifi_ok && say \"station still connected (\$(sta_conn)) with AP running\" || say 'WARNING: station lost connectivity with AP running'
sleep $secs
iw dev ap0 station dump 2>/dev/null | grep -c '^Station' | sed 's/^/[phone] associated stations: /' || true
sh \$RUN/restore.sh; disarm_watchdog
wait_wifi 30
dmesg | tail -50 | grep -i -E 'ath11k.*(crash|fail|assert|recovery)' || say 'no ath11k crash/recovery messages'"
  ;;

hotspot-nm)
  secs=${1:-60}; need_usb
  echo "network-test: hotspot-nm takes the client Wi-Fi down for ${secs}s (Lomiri semantics); USB SSH is the only link meanwhile."
  phone 1 "
saved=\$(sta_conn); say \"saved client connection: \${saved:-none}\"
cat > \$RUN/restore.sh <<EOF
nmcli connection down fp6-test-hotspot 2>/dev/null || true
nmcli connection delete fp6-test-hotspot 2>/dev/null || true
[ -n \"\$saved\" ] && nmcli connection up id \"\$saved\" || nmcli device connect wlan0 || true
EOF
arm_watchdog \$(( $secs + 90 ))
nmcli connection delete fp6-test-hotspot >/dev/null 2>&1 || true
nmcli connection add type wifi ifname wlan0 con-name fp6-test-hotspot autoconnect no ssid $SSID \
  802-11-wireless.mode ap 802-11-wireless.band bg ipv4.method shared ipv6.method ignore \
  wifi-sec.key-mgmt wpa-psk wifi-sec.proto rsn wifi-sec.pairwise ccmp wifi-sec.group ccmp wifi-sec.psk $PSK >/dev/null
if nmcli --wait 30 connection up fp6-test-hotspot; then
  iw dev wlan0 info | grep -E 'type|ssid|channel'
  ip -4 -br addr show wlan0
  pgrep -a dnsmasq | grep -i wlan0 | cut -c1-160 || say 'no NM dnsmasq for wlan0'
  nft list tables | grep -i nm-shared || say 'no nm-shared nftables table'
  nft list chain inet nixos-fw input-allow 2>/dev/null | grep -q 'wlan0' && say 'firewall admits DHCP/DNS on wlan0' || say 'firewall lacks wlan0 DHCP/DNS allowance: clients will not get leases until network-fp6.nix is deployed'
  say 'Hotspot up: connect a client to $SSID and browse (needs an uplink, e.g. mobile data, for internet).'
  sleep $secs
  nmcli -f GENERAL.STATE device show wlan0
  ip neigh show dev wlan0 | grep -c . | sed 's/^/[phone] neighbours on hotspot: /'
else
  say 'hotspot activation failed:'; journalctl -u NetworkManager -n 30 --no-pager | grep -v -i psk
fi
sh \$RUN/restore.sh; disarm_watchdog
wait_wifi 90"
  ;;

flight)
  secs=${1:-20}; need_usb
  phone 1 "
say 'before:'; radio_status
rfkill -n -o ID,TYPE,SOFT | awk '{print \$1, \$3}' > \$RUN/rfkill-before
saved=\$(sta_conn)
if busctl --system status org.freedesktop.URfkill >/dev/null 2>&1; then mode=urfkill; else mode=rfkill; fi
cat > \$RUN/restore.sh <<EOF
if [ \$mode = urfkill ]; then busctl --system call org.freedesktop.URfkill /org/freedesktop/URfkill org.freedesktop.URfkill FlightMode b false; fi
while read -r id soft; do [ \"\\\$soft\" = unblocked ] && rfkill unblock \"\\\$id\"; done < \$RUN/rfkill-before
EOF
arm_watchdog \$(( $secs + 90 ))
if [ \$mode = urfkill ]; then
  say 'flight mode ON via org.freedesktop.URfkill.FlightMode(true) (same call as the Lomiri switch)'
  busctl --system call org.freedesktop.URfkill /org/freedesktop/URfkill org.freedesktop.URfkill FlightMode b true
else
  say 'URfkill provider absent: emulating flight mode with rfkill block wlan bluetooth nfc'
  rfkill block wlan; rfkill block bluetooth; rfkill block nfc
fi
sleep 3; say 'during:'; radio_status
rfkill -n -o SOFT | grep -q unblocked && say 'WARNING: some radio still unblocked' || say 'all rfkill radios soft-blocked'
say 'modem: oFono must report Online=false here (see GetModems above)'
sleep $secs
sh \$RUN/restore.sh; disarm_watchdog
say 'after:'; radio_status
wait_wifi 90"
  ;;

nfc-poll)
  secs=${1:-20}
  [ "${FP6_NFC_RF_OK:-0}" = 1 ] || die "NFC RF power-on needs explicit authorization: rerun with FP6_NFC_RF_OK=1 and a test tag ready"
  phone 0 "
busctl --system status org.neard >/dev/null 2>&1 || { say 'neard not running: deploy network-fp6.nix with nfc.enable = true'; exit 1; }
A=/org/neard/nfc0
cat > \$RUN/restore.sh <<EOF
busctl --system call org.neard \$A org.neard.Adapter StopPollLoop 2>/dev/null || true
busctl --system set-property org.neard \$A org.neard.Adapter Powered b false || true
EOF
arm_watchdog \$(( $secs + 60 ))
busctl --system set-property org.neard \$A org.neard.Adapter Powered b true
busctl --system call org.neard \$A org.neard.Adapter StartPollLoop s Initiator
say \"polling for $secs s: hold an NFC tag to the back of the phone\"
end=\$(( \$(date +%s) + $secs )); found=
while [ \$(date +%s) -lt \$end ]; do
  found=\$(busctl --system tree org.neard 2>/dev/null | grep -o \"\$A/tag[0-9]*\" | head -1)
  [ -n \"\$found\" ] && break; sleep 1
done
if [ -n \"\$found\" ]; then
  say \"tag found: \$found\"
  for p in Type Protocol ReadOnly; do printf '  %s=' \$p; busctl --system get-property org.neard \$found org.neard.Tag \$p 2>/dev/null || echo '?'; done
  busctl --system tree org.neard | grep -c \"\$found/record\" | sed 's/^/  NDEF records: /'
else
  say 'no tag detected (check dmesg for s3fwrn5/nci errors)'
fi
sh \$RUN/restore.sh; disarm_watchdog
printf 'Powered after: '; busctl --system get-property org.neard \$A org.neard.Adapter Powered
dmesg | tail -30 | grep -i -E 's3fwrn5|nci' || true"
  ;;

*)
  sed -n '2,20p' "$0"; exit 2 ;;
esac
