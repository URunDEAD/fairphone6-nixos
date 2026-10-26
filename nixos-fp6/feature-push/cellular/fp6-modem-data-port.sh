#!/usr/bin/env bash
# FP6 (SM7635 / IPA v5.2) modem data path setup for oFono's qrtrsoc premux
# mode. Runtime QMI configuration only (no NV/EFS/PDC writes).
#
# Why: upstream oFono in premux mode assumes Android's dataservices already
# opened the IPA data port and set the data format. Without that, every
# WDS BindMuxDataPort fails with QMI "Internal" ("Failed to bind MUX") and
# WDA GetDataFormat(embedded, 1) returns InvalidArgument.
#
# 1. DPM OpenPort: hardware data endpoint embedded/iface 1 with the IPA
#    pipe pair from drivers/net/ipa/data/ipa_data-v5.2.c:
#      rx (IPA consumer, AP->modem) = IPA_ENDPOINT_AP_MODEM_TX = 2
#      tx (IPA producer, modem->AP) = IPA_ENDPOINT_AP_MODEM_RX = 18
#    (The swapped pair crashes the modem: ipa_sio.c IPA Assert dst_per_id.)
# 2. WDA SetDataFormat on that endpoint: raw-ip, QMAPv5 both directions
#    (IPA v5 endpoints use QMAP with the v5 checksum header).
# 3. "links" mode: (re)create rmnet_data0..N-1 (mux 1..N) with the matching
#    rmnet flags (deaggregation + MAPv5 checksum offload both ways).
#
# Usage: fp6-modem-data-port port      (steps 1+2; before ofonod)
#        fp6-modem-data-port links N   (step 3; before ofonod)
#        fp6-modem-data-port relink N  (after ofonod start: oFono's rmnet
#                                       init deletes every pre-existing rmnet
#                                       link, so wait for that, recreate)
set -euo pipefail

Q=(timeout -k 5 20 qmicli --silent -d qrtr://0)
mode="${1:-port}"
count="${2:-4}"

make_links() {
  modprobe rmnet || true
  for _ in $(seq 1 90); do
    [ -e /sys/class/net/rmnet_ipa0 ] && break
    sleep 1
  done
  if [ ! -e /sys/class/net/rmnet_ipa0 ]; then
    echo "rmnet_ipa0 absent; no rmnet links"
    return 1
  fi
  ip link set rmnet_ipa0 up || true
  for i in $(seq 0 $((count - 1))); do
    if [ ! -e "/sys/class/net/rmnet_data$i" ]; then
      ip link add link rmnet_ipa0 name "rmnet_data$i" type rmnet mux_id $((i + 1)) \
        ingress-deaggregation on ingress-mapv5-checksum on egress-mapv5-checksum on
      echo "created rmnet_data$i (mux $((i + 1)), QMAPv5 csum)"
    fi
  done
}

case "$mode" in
  port)
    for _ in $(seq 1 60); do
      "${Q[@]}" --dpm-noop >/dev/null 2>&1 && break
      sleep 1
    done
    if "${Q[@]}" --dpm-open-port="hw-data-ep-type=embedded,hw-data-ep-iface-number=1,hw-data-rx-id=2,hw-data-tx-id=18" >/dev/null 2>&1; then
      echo "DPM: opened IPA hardware data port (embedded/1, pipes 2/18)"
    else
      echo "DPM: open port failed (already open after an oFono restart is fine)"
    fi
    if "${Q[@]}" --wda-set-data-format="link-layer-protocol=raw-ip,ul-protocol=qmapv5,dl-protocol=qmapv5,dl-max-datagrams=32,dl-datagram-max-size=32768,ep-type=embedded,ep-iface-number=1" >/dev/null 2>&1; then
      echo "WDA: raw-ip + QMAPv5 on embedded/1"
    else
      echo "WDA: set data format failed; data contexts will not bind"
    fi
    ;;
  links)
    make_links
    ;;
  relink)
    # oFono's rmnet_init dumps and deletes existing rmnet links shortly after
    # start; the modem object keeps the premux names/mux ids.
    for _ in $(seq 1 15); do
      [ -e /sys/class/net/rmnet_data0 ] || break
      sleep 1
    done
    sleep 1
    make_links
    ;;
  *)
    echo "usage: $0 port | links N | relink N" >&2
    exit 2 ;;
esac
exit 0
