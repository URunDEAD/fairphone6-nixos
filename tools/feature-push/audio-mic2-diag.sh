#!/usr/bin/env bash
# FP6 top-mic (Mic2 = AMIC3 via WCD9378 ADC2) diagnosis. Run ON THE PHONE as
# the session user, on a generation with topMic = true:
#   tools/phone-ssh "cat > /tmp/fp6-mic2-diag.sh" < tools/feature-push/audio-mic2-diag.sh
#   tools/phone-ssh "bash /tmp/fp6-mic2-diag.sh" > results/feature-push-20261024/audio/live-mic2-diag.txt
# What it changes: switches the card to the highest "HiFi (Mic2...)" profile
# (UCM applies the Mic2 sequence), records, then switches back to the
# highest HiFi profile. It plays nothing. Root is used only to READ debugfs.
set -u
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
SUDO="env SUDO_ASKPASS=/home/user/fp6-install/sudo-askpass sudo -A -p ''"
T=$(mktemp -d /tmp/fp6-mic2.XXXXXX)
SLV="/sys/kernel/debug/soundwire/master-3-0/sdw:3:0:0217:0110:00:3/registers"
MST="/sys/kernel/debug/soundwire/master-3-0/qualcomm-sdw/qualcomm-registers"
CSR="/sys/kernel/debug/regmap/sdw:3:0:0217:0110:00:3-wcd9378_csr/registers"
TXM="/sys/kernel/debug/regmap/3220000.codec-tx_macro/registers"

profiles() { pw-dump | python3 -c '
import json,sys
for o in json.load(sys.stdin):
  if o.get("type","").endswith(":Device") and o["info"]["props"].get("device.name")=="alsa_card.platform-sound":
    print("ID", o["id"])
    for x in o["info"]["params"].get("EnumProfile",[]): print(x["index"], x["priority"], x["name"])
    for x in o["info"]["params"].get("Profile",[]): print("ACTIVE", x["name"])
'; }
pick() { profiles | awk -v pat="$1" '$1!="ID" && $1!="ACTIVE" { i=$1; p=$2; $1=$2=""; if ($0 ~ pat && (b=="" || p>bp)) {b=i; bp=p} } END {print b}'; }
snap() { # tag
  local t=$1
  sh -c "$SUDO cat '$SLV'" > "$T/slv-$t" 2>&1
  sh -c "$SUDO cat '$MST'" > "$T/mst-$t" 2>&1
  sh -c "$SUDO cat '$CSR'" > "$T/csr-$t" 2>&1
  sh -c "$SUDO cat '$TXM'" > "$T/txm-$t" 2>&1
  amixer -c F6 contents > "$T/amx-$t" 2>&1
  cat /proc/asound/card0/pcm1c/sub0/status > "$T/pcm-$t" 2>&1
}
seg_rms() { python3 - "$1" <<'PY'
import math,struct,sys
d=open(sys.argv[1],"rb").read(); n=len(d)//2
s=struct.unpack("<%dh"%n,d[:2*n]) if n else ()
seg=24000   # 0.5 s at 48 kHz mono
print("samples", n)
for k in range(0, n, seg):
  c=s[k:k+seg]; r=math.sqrt(sum(x*x for x in c)/len(c)) if c else 0
  print("t=%4.1fs rms=%7.1f dBFS zeros=%d/%d"%(k/48000, 20*math.log10(r/32768) if r else -999, sum(1 for x in c if x==0), len(c)))
PY
}

DEV=$(profiles | awk '$1=="ID"{print $2}')
echo "== profiles"; profiles
snap before
M2=$(pick "Mic2"); [ -n "$M2" ] || { echo "no Mic2 profile (topMic off?)"; exit 2; }
echo "== switching to Mic2 profile index $M2"; wpctl set-profile "$DEV" "$M2"; sleep 2
snap idle-mic2
echo "== capture 1: 8 s continuous; snapshots at 1 s and 6 s"
pw-record --raw --rate 48000 --channels 1 --format s16 "$T/c1.raw" & P=$!
sleep 1; snap c1-1s; sleep 5; snap c1-6s; sleep 2; kill -INT $P; wait $P 2>/dev/null
seg_rms "$T/c1.raw"
sleep 1
echo "== capture 2: 4 s after reopen"
pw-record --raw --rate 48000 --channels 1 --format s16 "$T/c2.raw" & P=$!
sleep 2; snap c2-2s; sleep 2; kill -INT $P; wait $P 2>/dev/null
seg_rms "$T/c2.raw"
echo "== kernel log since start"; sh -c "$SUDO dmesg" | tail -n 40 | grep -i -E "wcd|swr|soundwire|apm|tx_macro|asoc|dma" || echo "(none)"
for a in c1-1s c1-6s c2-2s; do
  for k in slv csr txm amx pcm; do
    echo "== diff $k idle-mic2 -> $a"; diff "$T/$k-idle-mic2" "$T/$k-$a" | head -40
  done
  echo "== diff mst idle-mic2 -> $a"; diff "$T/mst-idle-mic2" "$T/mst-$a" | head -60
done
echo "== full slave regs during good capture (c1-1s)"; cat "$T/slv-c1-1s"
B=$(pick "^ *HiFi"); echo "== restoring profile index $B"; wpctl set-profile "$DEV" "$B"
rm -rf "$T"
