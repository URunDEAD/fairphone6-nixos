#!/usr/bin/env bash
# Push the public repo only if the full-history secret scan is clean.
# Token: first line of $FP6_GH_TOKEN_FILE (default ~/Desktop/fp6-work/.github.env),
# optionally "GH_TOKEN=..." / "export GH_TOKEN=...". Never printed.
set -euo pipefail
repo=${FP6_PUBLIC:-$HOME/Desktop/fp6-nixos}
here=$(cd "$(dirname "$0")" && pwd)
if ! out=$("$here/secret-scan.sh" --history "$repo" 2>&1) || [[ $out != *"0 hits"* ]]; then
  printf '%s\n' "$out" | head -20 >&2
  echo "push-public: secret scan not clean; NOT pushing" >&2
  exit 1
fi
tf=${FP6_GH_TOKEN_FILE:-$HOME/Desktop/fp6-work/.github.env}
t=$(sed -n '1{s/^export //;s/^GH_TOKEN=//;s/[[:space:]]*$//;p}' "$tf")
[ -n "$t" ] || { echo "push-public: no token in $tf" >&2; exit 1; }
b64=$(printf 'x-access-token:%s' "$t" | base64 -w0); unset t
git -C "$repo" -c http.extraHeader="Authorization: Basic $b64" push origin "${1:-main}"
