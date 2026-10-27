#!/usr/bin/env bash
# Activate the FP6 modem's auto-selected platform MCFG (runtime only).
#
# Why: rmtfs runs read-only (-r), so modem EFS writes live only in rmtfs's
# RAM shadow. On every boot the modem therefore re-runs MCFG hardware
# auto-selection, leaves the chosen platform config "Pending" and waits for a
# reset. Until a platform config is "Active" the modem stays DMS "offline"
# and the UIM slots are not brought up (no ATR, eUICC invisible).
#
# What it does: polls PDC ListConfigs(platform) (read-only). If a config is
# Active: nothing to do. If one is Pending and none Active: restarts the
# remoteproc named "modem" once through sysfs (stop/start). rmtfs keeps
# running, so the RAM shadow holding the selection survives the restart.
# It never sends PDC SetSelectedConfig/ActivateConfig, NV or EFS writes.
set -euo pipefail

# PDC ListConfigs blocks while the modem is still loading MCFG files over
# TFTP, and frequent queries during that phase were seen to delay or stall
# the selection. So: settle first, then poll sparsely with a long bound.
#
# oFono (and so the phone UI's whole mobile network) is ordered after this
# unit, so every second spent here is "No network" after login. Boot
# journals of r2 (4 boots, 2026-10-27): the modem's MCFG TFTP reads end
# about 26 s after rmtfs starts it, Pending was already set at the first
# poll (60 s) every time, and Active at the first poll after the restart
# (60 s) every time; after the restart the modem's TFTP traffic is over
# within ~2 s. The old fixed 60 s + 60 s settles were ~65 s of pure wait.
# Override with MCFG_*_TIME (unit drop-in) if the stall ever comes back.
Q=(timeout -k 5 60 qmicli --silent -d qrtr://0)
SETTLE="${MCFG_SETTLE_TIME:-40}"
WAIT="${MCFG_WAIT_TIME:-240}"
POLL="${MCFG_POLL_TIME:-10}"
POST_SETTLE="${MCFG_POST_SETTLE_TIME:-15}"
POST_POLL="${MCFG_POST_POLL_TIME:-5}"

rproc=""
for r in /sys/class/remoteproc/remoteproc*; do
  if [ "$(cat "$r/name" 2>/dev/null)" = modem ]; then rproc=$r; break; fi
done
if [ -z "$rproc" ]; then
  echo "no modem remoteproc; nothing to do"
  exit 0
fi

platform_status() {
  "${Q[@]}" --pdc-list-configs=platform 2>/dev/null | grep -o "Status: *[A-Za-z]*" | awk '{print $2}' | sort -u | tr '\n' ' '
}

# The modem marks its auto-selected platform config "Pending" within 60 s
# of start (TFTP MCFG reads end at ~26 s). (DMS reports "shutting-down" from the start on this firmware,
# so it is not a usable trigger.)
sleep "$SETTLE"
st=""
deadline=$((SECONDS + WAIT))
while [ "$SECONDS" -lt "$deadline" ]; do
  st=$(platform_status || true)
  case " $st " in
    *" Active "*)
      echo "platform MCFG already active"
      exit 0 ;;
    *" Pending "*)
      break ;;
  esac
  sleep "$POLL"
done

case " $st " in
  *" Pending "*) ;;
  *)
    echo "no platform MCFG pending after $((SETTLE + WAIT)) s (pdc: ${st:-none}); leaving the modem alone"
    exit 0 ;;
esac

# Let the modem finish writing the selection into the (RAM) EFS.
sleep 10
echo "platform MCFG pending ($((SECONDS - 10)) s after start); restarting the modem remoteproc once to activate it"
echo stop > "$rproc/state"
for _ in $(seq 1 20); do
  [ "$(cat "$rproc/state")" = offline ] && break
  sleep 1
done
echo start > "$rproc/state"
for _ in $(seq 1 30); do
  [ "$(cat "$rproc/state")" = running ] && break
  sleep 1
done
t0=$SECONDS

sleep "$POST_SETTLE"
deadline=$((SECONDS + WAIT))
while [ "$SECONDS" -lt "$deadline" ]; do
  st=$(platform_status || true)
  case " $st " in
    *" Active "*)
      echo "platform MCFG active after modem restart ($((SECONDS - t0)) s after start)"
      exit 0 ;;
  esac
  sleep "$POST_POLL"
done
echo "platform MCFG still not active after restart (status: ${st:-none})"
exit 0
