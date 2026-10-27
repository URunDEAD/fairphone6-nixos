#!/usr/bin/env bash
# Needs nft, ip, nsenter/unshare, curl, python3 and a compiled netns_rules example.
# Runs entirely inside disposable network namespaces; no external interfaces.
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ ${1:-} != --inside ]]; then
  cargo build --example netns_rules
  if unshare --user --map-root-user --net true 2>/dev/null; then
    exec unshare --user --map-root-user --net bash "$0" --inside
  fi
  if [[ $EUID != 0 ]]; then
    echo 'User namespaces unavailable. Run as root with the build toolchain in PATH.' >&2
    exit 77
  fi
  exec unshare --net bash "$0" --inside
fi
# Use an existing ancestor; no cgroup write delegation is needed. Children retain
# this cgroup, and socket-cgroup rules only affect this isolated network namespace.
np_cgroup=$(awk -F: '$1 == 0 { print $3 }' /proc/self/cgroup)
np_cgroup=${np_cgroup#/}
# Match the top-level existing ancestor, so shell-generated names need no escaping.
np_cgroup=${np_cgroup%%/*}
[[ -n $np_cgroup ]] || { echo 'Need a non-root cgroup (run from a systemd scope)' >&2; exit 77; }
np_tmp=$(mktemp -d)
np_peer_pid=
cleanup() {
  if [[ -n $np_peer_pid ]]; then kill "$np_peer_pid" 2>/dev/null || true; wait "$np_peer_pid" 2>/dev/null || true; fi
  rm -rf "$np_tmp"
}
trap cleanup EXIT
unshare --net sleep 120 &
np_peer_pid=$!
sleep 0.2
ip link add np0 type veth peer name np1
ip link set np1 netns "$np_peer_pid"
ip addr add 198.18.0.1/30 dev np0
ip -6 addr add fd42:6::1/64 dev np0 nodad
ip link set lo up
ip link set np0 up
nsenter -t "$np_peer_pid" -n ip link set lo up
nsenter -t "$np_peer_pid" -n ip addr add 198.18.0.2/30 dev np1
nsenter -t "$np_peer_pid" -n ip -6 addr add fd42:6::2/64 dev np1 nodad
nsenter -t "$np_peer_pid" -n ip link set np1 up
# Synthetic fixture: only these test HTTP responses are transferred.
nsenter -t "$np_peer_pid" -n python3 -m http.server 18080 --bind :: --directory tests/golden > "$np_tmp/http.log" 2>&1 &
np_http_pid=$!
trap 'kill "$np_http_pid" 2>/dev/null || true; cleanup' EXIT
sleep 0.5
target/debug/examples/netns_rules "$np_cgroup" allow new > "$np_tmp/rules.nft"
if ! nft -f "$np_tmp/rules.nft"; then
  echo 'Kernel socket-cgroup matching denied/unavailable in this namespace; repeat as root on the phone.' >&2
  exit 77
fi
for addr in 198.18.0.2 '[fd42:6::2]'; do
  curl --noproxy '*' -fsS --max-time 3 "http://$addr:18080/background.nft" > /dev/null
done
nft -j list counters table inet fp6_netpolicy > "$np_tmp/before.json"
target/debug/examples/netns_rules "$np_cgroup" block update | nft -f -
for addr in 198.18.0.2 '[fd42:6::2]'; do
  if curl --noproxy '*' -fsS --max-time 1 "http://$addr:18080/background.nft" >/dev/null 2>&1; then
    echo 'FAIL: blocked app passed traffic' >&2; exit 1
  fi
done
nft -j list counters table inet fp6_netpolicy > "$np_tmp/blocked.json"
python3 - "$np_tmp/before.json" "$np_tmp/blocked.json" <<'PY'
import json, sys
def counters(path):
    return {x['counter']['name']: x['counter']['bytes'] for x in json.load(open(path))['nftables'] if 'counter' in x}
a, b = map(counters, sys.argv[1:])
assert a['a74657374_tx'] > 0 and a['a74657374_rx'] > 0, a
assert a['a74657374_tx'] == b['a74657374_tx'], (a, b)
assert b['a74657374_rx'] >= a['a74657374_rx'], (a, b)
PY
target/debug/examples/netns_rules "$np_cgroup" allow update | nft -f -
curl --noproxy '*' -fsS --max-time 3 http://198.18.0.2:18080/background.nft > /dev/null
echo 'PASS: IPv4/IPv6 allow, block, restore, TX/RX accounting and counter retention'
