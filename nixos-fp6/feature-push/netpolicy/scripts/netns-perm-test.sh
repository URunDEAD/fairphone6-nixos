#!/usr/bin/env bash
# App network permission (app-permissions) in disposable network namespaces:
# Wi-Fi/other denied with local network allowed, local denied, mobile-only
# denial on a non-mobile link, no network at all, and removal of the
# restriction (stale chain deleted). Same requirements as netns-test.sh;
# exit 77 = unsupported here.
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
np_cgroup=$(awk -F: '$1 == 0 { print $3 }' /proc/self/cgroup)
np_cgroup=${np_cgroup#/}
np_cgroup=${np_cgroup%%/*}
[[ -n $np_cgroup ]] || { echo 'Need a non-root cgroup (run from a systemd scope)' >&2; exit 77; }
np_tmp=$(mktemp -d)
np_peer_pid=
np_http_pid=
cleanup() {
  for p in $np_http_pid $np_peer_pid; do kill "$p" 2>/dev/null || true; wait "$p" 2>/dev/null || true; done
  rm -rf "$np_tmp"
}
trap cleanup EXIT
unshare --net sleep 120 &
np_peer_pid=$!
sleep 0.2
# np0: a Wi-Fi-like link with an "internet" address (198.18.0.2) and a LAN
# address (192.168.77.2) behind it.
ip link add np0 type veth peer name np1
ip link set np1 netns "$np_peer_pid"
ip addr add 198.18.0.1/30 dev np0
ip addr add 192.168.77.1/24 dev np0
ip -6 addr add fd42:6::1/64 dev np0 nodad
ip link set lo up
ip link set np0 up
nsenter -t "$np_peer_pid" -n ip link set lo up
nsenter -t "$np_peer_pid" -n ip addr add 198.18.0.2/30 dev np1
nsenter -t "$np_peer_pid" -n ip addr add 192.168.77.2/24 dev np1
nsenter -t "$np_peer_pid" -n ip -6 addr add fd42:6::2/64 dev np1 nodad
nsenter -t "$np_peer_pid" -n ip link set np1 up
nsenter -t "$np_peer_pid" -n python3 -m http.server 18080 --bind :: --directory tests/golden > "$np_tmp/http.log" 2>&1 &
np_http_pid=$!
sleep 0.5
rules() { target/debug/examples/netns_rules "$np_cgroup" perm "$@"; }
get() { curl --noproxy '*' -fsS --max-time 1 "http://$1:18080/background.nft" >/dev/null 2>&1; }
expect() {  # expect NAME internet-v4 lan-v4 ula-v6 (1 = reachable)
  local name=$1 r
  shift
  for pair in "198.18.0.2:$1" "192.168.77.2:$2" "[fd42:6::2]:$3"; do
    if get "${pair%:*}"; then r=1; else r=0; fi
    if [[ $r != "${pair##*:}" ]]; then echo "FAIL: $name: ${pair%:*} reachable=$r" >&2; exit 1; fi
  done
  echo "PASS: $name"
}
expect "no table" 1 1 1
# wifi=0 mobile=1 vpn=1 local=1
if ! rules 0 1 1 1 new | nft -f -; then
  echo 'Kernel socket-cgroup matching denied/unavailable in this namespace; repeat as root on the phone.' >&2
  exit 77
fi
expect "Wi-Fi denied, local network allowed" 0 1 1
rules 1 1 1 0 1 | nft -f -
expect "local network denied" 1 0 0
rules 1 0 1 1 1 | nft -f -
expect "mobile denied (np0 is not mobile)" 1 1 1
rules 0 0 0 0 1 | nft -f -
expect "no network at all" 0 0 0
rules 1 1 1 1 1 | nft -f -
expect "restriction removed" 1 1 1
if nft list chain inet fp6_netpolicy perm_0_out >/dev/null 2>&1; then
  echo 'FAIL: stale permission chain left behind' >&2; exit 1
fi
echo 'PASS: app network permission (Wi-Fi/local/mobile/none/removal, IPv4+IPv6)'
