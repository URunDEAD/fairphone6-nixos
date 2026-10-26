#!/usr/bin/env bash
# FP6 port of postmarketOS msm-modem-uim-selection (as shipped in the old
# /run/nura root), reduced to the QRTR path and WITHOUT qmi-proxy.
#
# What it does: if a UICC with a USIM application is present, activate the
# primary GW provisioning session on the first present slot so the modem
# owner (oFono qrtrqmi) can read the SIM. This is a runtime UIM session
# selection; it is not an NV edit, not a SIM-lock/carrier-lock operation and
# it never sends a PIN. Identifiers (ICCID/AID) are never printed.
#
# eUICC handling: right after a profile was enabled (or on some boots) the
# eUICC slot reports a present card without a USIM application until the
# slot is power-cycled. If no USIM app shows up within USIM_WAIT_TIME the
# script power-cycles that slot once (UIM PowerOffSIM/PowerOnSIM, runtime
# only) and looks again.
#
# Writes performed: optional slot power off/on (once), at most one
# ChangeProvisioningSession(deactivate) and one (activate). Nothing else.
# Exit status: 0 when a session was activated or there is nothing to do,
# 75 (EX_TEMPFAIL) when a card is present but no USIM app appeared, so
# systemd's Restart=on-failure retries later.
set -uo pipefail

SIM_WAIT_TIME="${SIM_WAIT_TIME:-30}"
USIM_WAIT_TIME="${USIM_WAIT_TIME:-15}"
Q=(timeout -k 5 20 qmicli --silent -d qrtr://0)

# shellcheck disable=SC2329  # invoked via trap
ready() {
  if command -v systemd-notify >/dev/null; then systemd-notify --ready || true; fi
}
trap ready EXIT

card_status() { "${Q[@]}" --uim-get-card-status 2>/dev/null || true; }
# First slot number whose card is present (empty if none).
present_slot() {
  printf '%s\n' "$1" | awk '/^Slot \[[0-9]+\]:/ {gsub(/[^0-9]/, "", $2); s=$2} /Card state: .present./ {print s; exit}'
}
# AID of the first USIM application (empty if none). Never printed.
usim_aid() {
  printf '%s\n' "$1" | grep -A3 -m1 "usim (2)" | tail -n1 | awk '{print $1}' || true
}

count=0
until "${Q[@]}" --uim-noop >/dev/null 2>&1; do
  count=$((count + 1))
  if [ "$count" -ge 45 ]; then
    echo "QRTR UIM service did not appear after 45 s; leaving the modem untouched."
    exit 0
  fi
  sleep 1
done
echo "QRTR UIM service up after ${count} s"

cards=$(card_status)
count=0
slot=$(present_slot "$cards")
while [ -z "$slot" ]; do
  if [ "$count" -ge "$SIM_WAIT_TIME" ]; then
    echo "No UICC reported present after ${SIM_WAIT_TIME} s."
    exit 0
  fi
  sleep 1
  count=$((count + 1))
  cards=$(card_status)
  slot=$(present_slot "$cards")
done
echo "UICC present in slot ${slot} after ${count} s"

aid=""
for attempt in 1 2; do
  for _ in $(seq 1 "$USIM_WAIT_TIME"); do
    aid=$(usim_aid "$cards")
    [ -n "$aid" ] && break 2
    sleep 1
    cards=$(card_status)
  done
  if [ "$attempt" = 1 ]; then
    echo "Card in slot ${slot} lists no USIM application; power-cycling the slot once"
    "${Q[@]}" --uim-sim-power-off="$slot" >/dev/null 2>&1 || true
    sleep 2
    "${Q[@]}" --uim-sim-power-on="$slot" >/dev/null 2>&1 || true
    sleep 3
    cards=$(card_status)
    slot2=$(present_slot "$cards")
    [ -n "$slot2" ] && slot=$slot2
  fi
done

if [ -z "$aid" ]; then
  echo "No USIM application on the present card (empty eUICC or no profile enabled); will retry."
  exit 75
fi

if ! printf '%s' "$cards" | grep -Fq "Primary GW:   session doesn't exist"; then
  echo "Primary GW provisioning session already exists; resetting it once."
  "${Q[@]}" --uim-change-provisioning-session='activate=no,session-type=primary-gw-provisioning' >/dev/null 2>&1 || true
fi

echo "Activating primary GW provisioning session on slot ${slot} (AID redacted)"
if ! "${Q[@]}" --uim-change-provisioning-session="slot=${slot},activate=yes,session-type=primary-gw-provisioning,aid=${aid}" >/dev/null 2>&1; then
  echo "Provisioning session activation failed; will retry."
  exit 75
fi
exit 0
