#!/usr/bin/env bash
# Decode test (synthetic networks only): the fp6-wifi-qr CLI writes QR PNGs,
# then they are decoded by our decoder (rqrr) and by two independent ones
# (zbarimg, zxing-cpp); and codes written by another encoder (qrencode) are
# decoded by ours. Each decoded payload must equal the encoded one.
#   test/decode-test.sh CORE_STORE_PATH OUTDIR
set -euo pipefail
core=$1 out=$2
mkdir -p "$out"
cli=$core/bin/fp6-wifi-qr
tools=$(nix-build --no-out-link -E 'with import (import '"$(cd "$(dirname "$0")"/../../../upstream/mobile-nixos/npins && pwd)"') .nixpkgs {}; symlinkJoin { name = "qr-tools"; paths = [ zbar.out qrencode (python3.withPackages (p: [ p.zxing-cpp p.pillow ])) ]; }' 2>/dev/null) || true
fail=0
n=0
while IFS='|' read -r ssid sec pw hidden; do
  n=$((n + 1))
  args=("$ssid" "$sec"); [ -n "$pw" ] && args+=("$pw"); [ "$hidden" = 1 ] && args+=(--hidden)
  payload=$("$cli" encode "${args[@]}")
  png=$out/code-$n.png
  "$cli" png "$payload" "$png" 6 >/dev/null
  if "$cli" decode "$png" --expect "$payload" >/dev/null; then r1=ok; else r1=FAIL; fail=1; fi
  r2=skip; r3=skip; r4=skip
  if [ -x "$tools/bin/zbarimg" ]; then
    got=$("$tools/bin/zbarimg" -q --raw -Sbinary "$png" 2>/dev/null || true)
    [ "$got" = "$payload" ] && r2=ok || { r2=FAIL; fail=1; }
  fi
  if [ -x "$tools/bin/python3" ]; then
    got=$("$tools/bin/python3" -I -c 'import sys, zxingcpp; from PIL import Image
r = zxingcpp.read_barcodes(Image.open(sys.argv[1])); sys.stdout.write(r[0].text if r else "")' "$png")
    [ "$got" = "$payload" ] && r3=ok || { r3="FAIL"; fail=1; }
  fi
  if [ -x "$tools/bin/qrencode" ]; then
    "$tools/bin/qrencode" -s 5 -l M -o "$out/qrencode-$n.png" "$payload"
    "$cli" decode "$out/qrencode-$n.png" --expect "$payload" >/dev/null && r4=ok || { r4=FAIL; fail=1; }
  fi
  printf '%-34s %-6s ours:%-4s zbar:%-4s zxing-cpp:%-4s qrencode->ours:%s\n' "$ssid" "$sec" "$r1" "$r2" "$r3" "$r4"
done <<'CASES'
Test;Net|WPA|p@ss:word,1|0
Lab Net|SAE|longpassword|1
Cafe "Guest"|nopass||0
Back\slash, colon:|WPA|a;b:c,d\e"f|0
Ünïcødé 📶|SAE|s|0
Old|WEP|abcde|1
CASES
exit $fail
