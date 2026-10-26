#!/usr/bin/env bash
# FP6 cellular + GNSS staged live bring-up (oFono over QRTR, Lomiri target).
#
# Run from <workspace> on the PC. Every step talks to
# the phone only through tools/phone-ssh. Steps are separate subcommands so
# the parent can stop at any checkpoint:
#
#   preflight   read-only: kernel, remoteproc, rmtfs flags, owners, links
#   nv-before   read-only SHA-256 of modemst1/modemst2/fsg/fsc/study (baseline)
#   sim         read-only UIM card status (identifiers redacted)
#   ofono       read-only: oFono modems / interfaces / SIM / registration
#   radio-on    ACTIVE (needs --yes): oFono Modem Powered=true, Online=true
#   pin         ACTIVE (needs --yes): ONE EnterPin attempt, PIN read from tty,
#               refused unless oFono reports >= 2 PIN retries left
#   register    read-only: operator name, technology, strength (no cell ids)
#   data        ACTIVE (needs --yes): attach/activate internet context, one
#               bounded 1 MiB HTTPS fetch over the cellular iface, deactivate
#   gnss        ACTIVE (needs --yes): one on-demand LOC session for N s;
#               prints fix yes/no + satellites, NEVER coordinates
#   nv-after    read-only re-hash; compares with nv-before; STOP on mismatch
#   tests       prints the manual call / SMS / MMS / VoLTE test plan (no action)
#   all-readonly  preflight nv-before sim ofono register nv-after
#
# NEVER: dials, sends SMS/MMS, guesses PINs, touches IMEI/NV items/PDC/MCFG,
# carrier or SIM lock, eSIM profiles, writes raw partitions, writes
# remoteproc state, reboots or uses fastboot. Placing calls/SMS is the
# parent's job with a user-supplied test number (see `tests`).
set -euo pipefail

cd "$(dirname "$0")/../.."
PHONE=tools/phone-ssh
SUDO='SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p ""'
QMICLI=${FP6_QMICLI:-}
OUT=results/feature-push-20261024/cellular/bringup
mkdir -p "$OUT"
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
YES=0
GNSS_SECONDS=300
for a in "$@"; do
  case "$a" in
    --yes) YES=1 ;;
    --gnss-seconds=*) GNSS_SECONDS=${a#*=} ;;
  esac
done

p() { "$PHONE" "$1"; }
if [ -z "$QMICLI" ]; then
  QMICLI=$("$PHONE" 'command -v qmicli || ls -d /nix/store/*-libqmi-1.*/bin/qmicli 2>/dev/null | head -n1' | tail -n1)
fi
need_yes() {
  if [ "$YES" != 1 ]; then
    echo "REFUSED: '$1' changes radio/modem runtime state; re-run with --yes after user approval." >&2
    exit 3
  fi
}
redact() { sed -E 's/[0-9A-Fa-f]{10,}/<redacted>/g; s/("?(SubscriberIdentity|CardIdentifier|Serial|SubscriberNumbers|ServiceProviderName|CellId|LocationAreaCode|Imei|MobileCountryCode|MobileNetworkCode)"?[^,}]*)/\2=<redacted>/g'; }
log() { echo "== [$1] $(date -u +%H:%M:%SZ)"; }

ofono_props() { # $1 path $2 interface  -> JSON
  p "busctl --system --json=short call org.ofono '$1' '$2' GetProperties 2>&1" ; }

first_modem() {
  p "busctl --system --json=short call org.ofono / org.ofono.Manager GetModems 2>/dev/null" |
    python3 -I -c 'import json,sys
t=sys.stdin.read().strip()
d=json.loads(t)["data"][0] if t else []
print(d[0][0] if d else "")'
}

step_preflight() {
  log preflight
  p "uname -r; for r in /sys/class/remoteproc/remoteproc*; do echo \$(cat \$r/name) \$(cat \$r/state); done"
  p "systemctl show rmtfs -p ExecStart --value | grep -o 'rmtfs [^;]*'"
  if ! p "systemctl show rmtfs -p ExecStart --value" | grep -q 'rmtfs -r '; then
    echo "STOP: rmtfs is not read-only (-r). Do not continue." >&2; exit 2
  fi
  p "for u in rmtfs tqftpserv ModemManager ofono msm-modem-uim-selection fp6-gnss-nmea geoclue NetworkManager; do printf '%-26s %s\n' \$u \$(systemctl is-active \$u 2>/dev/null); done"
  if p "systemctl is-active ModemManager" 2>/dev/null | grep -qx active; then
    echo "STOP: ModemManager is active alongside the oFono stack (two modem owners)." >&2; exit 2
  fi
  p "ip -br link | grep -E 'rmnet' || echo 'no rmnet links'"
  p "$QMICLI -d qrtr://0 --dms-get-operating-mode 2>&1 | grep -E 'Mode|restricted'" || true
}

nv_hash() { # $1 label
  p "$SUDO sh -c 'for n in modemst1 modemst2 fsg fsc study; do d=/dev/disk/by-partlabel/\$n; [ -e \$d ] && printf \"%s %s %s\n\" \$n \$(blockdev --getsize64 \$d) \$(sha256sum < \$d | cut -d\" \" -f1); done'" \
    | tee "$OUT/nv-$1-$STAMP.txt"
  cp "$OUT/nv-$1-$STAMP.txt" "$OUT/nv-$1-latest.txt"
}

step_nv_before() {
  log nv-before
  echo "Read-only SHA-256 of rmtfs backing partitions (sha256sum opens O_RDONLY)."
  nv_hash before
}

step_nv_after() {
  log nv-after
  nv_hash after
  if [ ! -f "$OUT/nv-before-latest.txt" ]; then echo "no baseline; run nv-before first" >&2; exit 2; fi
  if diff -u "$OUT/nv-before-latest.txt" "$OUT/nv-after-latest.txt"; then
    echo "PASS: modemst1/modemst2/fsg/fsc/study unchanged (rmtfs -r shadowing held)."
  else
    echo "STOP: rmtfs backing partitions CHANGED. Stop rmtfs-dependent work, do not reboot casually, preserve logs, report to user." >&2
    exit 2
  fi
}

step_sim() {
  log sim
  p "$QMICLI -d qrtr://0 --uim-get-card-status 2>&1" | grep -E "Slot|Card state|UPIN state|retries|Primary GW|Application state|usim|PIN1 state|PIN1 retries" | redact
  echo "Interpretation: 'no-atr-received' on both slots = no readable physical SIM (seen on every pmOS and NixOS boot so far)."
}

step_ofono() {
  log ofono
  m=$(first_modem || true)
  if [ -z "$m" ]; then echo "oFono reports no modem (check: ofono ExecStartPre/Post (fp6-modem-data-port) created rmnet_data0..3; journalctl -u ofono)."; return 0; fi
  echo "modem path: $m"
  ofono_props "$m" org.ofono.Modem | redact
  ofono_props "$m" org.ofono.SimManager | redact || true
  ofono_props "$m" org.ofono.NetworkRegistration | redact || true
  ofono_props "$m" org.ofono.ConnectionManager | redact || true
}

step_radio_on() {
  need_yes radio-on
  log radio-on
  m=$(first_modem); [ -n "$m" ] || { echo "no oFono modem" >&2; exit 2; }
  p "busctl --system call org.ofono '$m' org.ofono.Modem SetProperty sv Powered b true"
  sleep 5
  p "busctl --system call org.ofono '$m' org.ofono.Modem SetProperty sv Online b true"
  sleep 10
  ofono_props "$m" org.ofono.Modem | redact
}

step_pin() {
  need_yes pin
  log pin
  m=$(first_modem); [ -n "$m" ] || { echo "no oFono modem" >&2; exit 2; }
  props=$(ofono_props "$m" org.ofono.SimManager)
  req=$(printf '%s' "$props" | python3 -I -c 'import json,sys; d=json.load(sys.stdin)["data"][0]; print(d.get("PinRequired",{}).get("data","none"))')
  left=$(printf '%s' "$props" | python3 -I -c 'import json,sys; d=json.load(sys.stdin)["data"][0]; print(d.get("Retries",{}).get("data",{}).get("pin",-1))')
  echo "PinRequired=$req pin-retries-left=$left"
  [ "$req" = pin ] || { echo "No PIN required (or PUK/other: $req). Nothing done. PUK is NOT handled here."; return 0; }
  [ "$left" -ge 2 ] || { echo "REFUSED: fewer than 2 PIN retries left; ask the user to unlock in the Lomiri UI themselves." >&2; exit 3; }
  read -r -s -p "SIM PIN (one attempt, typed by the user): " pin; echo
  printf '%s' "$pin" | "$PHONE" "read -r x; busctl --system call org.ofono '$m' org.ofono.SimManager EnterPin ss pin \"\$x\""
  unset pin
}

step_register() {
  log register
  m=$(first_modem); [ -n "$m" ] || { echo "no oFono modem" >&2; exit 2; }
  ofono_props "$m" org.ofono.NetworkRegistration | python3 -I -c 'import json,sys
d=json.load(sys.stdin)["data"][0]
for k in ("Status","Name","Technology","Strength","Mode"):
    if k in d: print(f"{k}={d[k]['"'"'data'"'"']}")'
}

step_data() {
  need_yes data
  log data
  m=$(first_modem); [ -n "$m" ] || { echo "no oFono modem" >&2; exit 2; }
  ofono_props "$m" org.ofono.ConnectionManager | redact
  p "busctl --system --json=short call org.ofono '$m' org.ofono.ConnectionManager GetContexts" > "$OUT/contexts-$STAMP.json"
  ctx=$(python3 -I -c 'import json,sys
for path,props in json.load(open(sys.argv[1]))["data"][0]:
    if props.get("Type",{}).get("data")=="internet": print(path); break' "$OUT/contexts-$STAMP.json")
  [ -n "$ctx" ] || { echo "no internet context provisioned (mobile-broadband-provider-info); set APN via Lomiri settings" >&2; exit 2; }
  echo "activating $ctx (via NetworkManager's oFono device if present, else oFono directly)"
  p "nmcli -t -f DEVICE,TYPE,STATE device | grep -E 'gsm|modem' || true"
  p "busctl --system call org.ofono '$ctx' org.ofono.ConnectionContext SetProperty sv Active b true"
  sleep 8
  iface=$(p "busctl --system --json=short call org.ofono '$ctx' org.ofono.ConnectionContext GetProperties" |
    python3 -I -c 'import json,sys; d=json.load(sys.stdin)["data"][0]; print(d.get("Settings",{}).get("data",{}).get("Interface",{}).get("data",""))')
  echo "context interface: ${iface:-<none>}"
  p "ip -br addr show ${iface:-rmnet_data0}; ip route | grep -E 'rmnet' || true"
  if [ -n "$iface" ]; then
    p "curl -sS --interface $iface -m 30 -o /dev/null -w 'http=%{http_code} bytes=%{size_download} t=%{time_total}s\n' https://speed.cloudflare.com/__down?bytes=1048576 || echo 'transfer failed'"
  fi
  p "busctl --system call org.ofono '$ctx' org.ofono.ConnectionContext SetProperty sv Active b false" || true
}

step_gnss() {
  need_yes gnss
  log gnss
  echo "Read-only engine-lock query first (never changed by this script):"
  p "$QMICLI -d qrtr://0 --loc-get-engine-lock 2>&1 | tail -n +2"
  echo "One LOC session for ${GNSS_SECONDS}s (phone outdoors / near a window); coordinates are discarded on the phone."
  p "timeout ${GNSS_SECONDS} /run/current-system/sw/bin/fp6-loc-nmea 2>/dev/null | awk -F, '
      /^\\\$G.GGA/ { if (\$7+0 > 0) { fix=1; q=\$7; sats=\$8 } }
      /^\\\$G.RMC/ { if (\$3==\"A\") rmc=1 }
      /^\\\$G.GSV/ { split(\$1,a,\"GSV\"); seen[a[1]]=\$4 }
      { n++ }
      END { printf \"nmea_lines=%d gga_fix=%s quality=%s sats_used=%s rmc_valid=%s\\n\", n, fix?\"yes\":\"no\", q, sats, rmc?\"yes\":\"no\";
            for (c in seen) printf \"in_view[%s]=%s\\n\", c, seen[c] }'"
}

step_tests() {
  cat <<'EOF'
MANUAL TESTS (parent + user; this script never performs them)
Prereqs: registered (step register), user-provided TEST NUMBER, user present.
1 Outgoing call: Lomiri dialer -> test number. Expect ring-back, connect,
  two-way audio. NOTE: FP6 UCM has no "Voice Call" verb yet -> audio likely
  absent; record signalling result separately from audio.
2 Incoming call: test number calls the phone. Expect Lomiri incoming-call UI,
  answer, hang-up from both ends; history-service entry.
3 SMS out/in: send "fp6 test <time>" to the test number; reply from it.
  Expect delivery + messaging-app thread; check journal for ofono WMS errors.
4 MMS out/in (nuntium): needs a provisioned MMS context (APN/MMSC/proxy).
  Send a small JPEG to the test number, then receive one. Check
  `journalctl --user -u nuntium`, ofono PushNotification agent registration.
5 VoLTE: Lomiri/oFono shows no IMS atom on qrtrqmi -> calls are CS/CSFB only.
  If the carrier has no 2G/3G, calls will fail; that is the documented gap.
6 SIM PIN: user enters PIN in the Lomiri unlock dialog (or step `pin`), never
  guessed. PUK flows are user-only.
Afterwards run: nv-after (must PASS).
EOF
}

cmd=${1:-help}
case "$cmd" in
  preflight) step_preflight ;;
  nv-before) step_nv_before ;;
  nv-after) step_nv_after ;;
  sim) step_sim ;;
  ofono) step_ofono ;;
  radio-on) step_radio_on ;;
  pin) step_pin ;;
  register) step_register ;;
  data) step_data ;;
  gnss) step_gnss ;;
  tests) step_tests ;;
  all-readonly) step_preflight; step_nv_before; step_sim; step_ofono; step_register || true; step_nv_after ;;
  *) sed -n '2,30p' "$0"; exit 0 ;;
esac 2>&1 | tee -a "$OUT/bringup-$STAMP.log"
