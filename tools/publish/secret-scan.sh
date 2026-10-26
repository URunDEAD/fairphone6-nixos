#!/usr/bin/env bash
# Secret and privacy scan for the public FP6 tree.
#
# Usage:
#   tools/publish/secret-scan.sh <dir>              scan the files of a directory
#   tools/publish/secret-scan.sh --history <repo>   scan every patch in a repo's history
#   tools/publish/secret-scan.sh --files <list>     scan the files named in <list>
#
# Generic patterns live in this script. Literal private values (the phone
# password, builder host/user, Wi-Fi SSIDs, ...) must NOT be written here:
# put one extended regex per line into an untracked file and point
# FP6_SECRET_PATTERNS at it (default: ~/.config/fp6-publish/secret-patterns.txt).
# Lines starting with '#' are ignored.
#
# Known-harmless matches (regexes, one per line) can be listed in
# tools/publish/scan-allow.txt next to this script.
#
# Exit status: 0 = no hits, 1 = hits found, 2 = usage error.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
mode=dir
case "${1-}" in
  --history) mode=history; shift ;;
  --files) mode=files; shift ;;
  -h|--help|'') sed -n '2,18p' "$0"; exit 2 ;;
esac
target=$1

# ---- generic patterns (extended regex, case-insensitive) ----
patterns=(
  # credentials and keys
  'BEGIN [A-Z ]*PRIVATE KEY'
  'ssh-(ed25519|rsa|dss) AAAA'
  'ecdsa-sha2-nistp[0-9]+ AAAA'
  'ssh-ng://'
  'base64-ssh-public-host-key'
  'ssh-key=%2F'
  'SSHPASS=[^"$ ]'
  '[A-Z]+_(PASS|PASSWORD|TOKEN|SECRET)=["'"'"']?[A-Za-z0-9+/_-]{6,}'
  '(password|passwd|passphrase)[[:space:]]*[:=][[:space:]]*"?[A-Za-z0-9]{4,}'
  'initialPassword|hashedPassword[[:space:]]*='
  'gh[pousr]_[A-Za-z0-9]{30,}'
  'glpat-[A-Za-z0-9_-]{20,}'
  'xox[abp]-[A-Za-z0-9-]{10,}'
  'AKIA[0-9A-Z]{16}'
  # private network addresses (USB-net 172.16.42.x, Waydroid 192.168.240.x and
  # QEMU 10.0.2.x defaults are allowed via scan-allow.txt)
  '(^|[^0-9.])192\.168\.[0-9]{1,3}\.[0-9]{1,3}'
  '(^|[^0-9.])10\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}([^0-9.]|$)'
  '(^|[^0-9.])100\.(6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7])\.[0-9]{1,3}\.[0-9]{1,3}'
  '(^|[^0-9.])172\.(1[6-9]|2[0-9]|3[01])\.[0-9]{1,3}\.[0-9]{1,3}'
  # personal paths and accounts
  '/home/[a-z_][a-z0-9_-]*/(Desktop|Documents|\.ssh|\.config)'
  '/Users/[A-Za-z0-9_-]+/'
  '[A-Za-z0-9._%+-]+@gmail\.com'
  # mobile identifiers
  '(imei|iccid|imsi|eid|msisdn)[^A-Za-z0-9]{1,6}[0-9]{14,32}'
  '(^|[^0-9])35[0-9]{13}([^0-9]|$)'
  '(^|[^0-9])89[0-9]{17,18}([^0-9]|$)'
  '(^|[^0-9])89[0-9]{30}([^0-9]|$)'
  '(^|[^0-9a-z])(\+|00)(40|49|33|31|44|1)[0-9 ]{9,12}([^0-9]|$)'
  # eSIM activation codes
  'LPA:1\$'
  'sm-?dp\+?[^A-Za-z]{1,4}[A-Za-z0-9.-]+\.[a-z]{2,}'
  # location
  'geo:-?[0-9]{1,2}\.[0-9]{3,}'
  '(^|[^0-9.])-?[0-9]{1,2}\.[0-9]{5,}[[:space:]]*,[[:space:]]*-?[0-9]{1,3}\.[0-9]{5,}[^0-9]*$'
  '(lat|latitude|lon|lng|longitude)"?[[:space:]]*[:=][[:space:]]*-?[0-9]{1,3}\.[0-9]{4,}'
  # Wi-Fi
  '(^|[^a-z])(ssid|psk)[[:space:]]*=[[:space:]]*[^[:space:]$]+'
)

extra=${FP6_SECRET_PATTERNS:-$HOME/.config/fp6-publish/secret-patterns.txt}
if [ -r "$extra" ]; then
  while IFS= read -r line; do
    case "$line" in ''|'#'*) continue ;; esac
    patterns+=("$line")
  done < "$extra"
else
  echo "secret-scan: note: no literal pattern file at $extra (generic patterns only)" >&2
fi

allow=()
if [ -r "$here/scan-allow.txt" ]; then
  while IFS= read -r line; do
    case "$line" in ''|'#'*) continue ;; esac
    allow+=("$line")
  done < "$here/scan-allow.txt"
fi

# case-sensitive: eSIM matching IDs / activation codes like ABCD-1234-EFGH-5678
cs_patterns=(
  '(^|[^0-9A-Za-z_./-])[0-9A-Z]*[0-9][0-9A-Z]*(-[0-9A-Z]{4,8}){3,}([^0-9A-Za-z_-]|$)'
  '(^|[^0-9A-Za-z])[0-9A-F]{32,}([^0-9A-Za-z]|$)'
)
cs_file=$(mktemp)
printf '%s\n' "${cs_patterns[@]}" > "$cs_file"
pattern_file=$(mktemp); allow_file=$(mktemp); hits_file=$(mktemp)
trap 'rm -f "$pattern_file" "$allow_file" "$hits_file" "$cs_file"' EXIT
printf '%s\n' "${patterns[@]}" > "$pattern_file"
: > "$allow_file"; : > "$hits_file"
if [ ${#allow[@]} -gt 0 ]; then printf '%s\n' "${allow[@]}" > "$allow_file"; fi

# grep_both ARGS... : case-insensitive generic pass + case-sensitive pass
grep_both() {
  { grep -i -E -f "$pattern_file" "$@"; grep -E -f "$cs_file" "$@"; } || true
}
filter_allowed() {
  if [ -s "$allow_file" ]; then grep -v -i -E -f "$allow_file" || true; else cat; fi
}

case "$mode" in
  dir)
    if git -C "$target" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
      # tracked + untracked-but-not-ignored files (skips fetched upstreams/assets)
      (cd "$target" && git ls-files -z -co --exclude-standard \
         | grep -z -v -E '(^|/)(secret-scan\.sh|scan-allow\.txt)$' \
         | while IFS= read -r -d '' f; do [ -f "$f" ] && grep_both -I -n -H -- "$f"; done) \
        | filter_allowed > "$hits_file"
    else
      (cd "$target" && grep_both -r -I -n \
          --exclude-dir=.git --exclude=secret-scan.sh --exclude=scan-allow.txt .) \
        | filter_allowed > "$hits_file"
    fi
    ;;
  files)
    while IFS= read -r f; do [ -f "$f" ] && grep_both -I -n -H -- "$f"; done < "$target" \
      | filter_allowed > "$hits_file" || true
    ;;
  history)
    # every added/removed line of every commit, plus commit metadata
    git -C "$target" log --all -p --format='commit %H%nAuthor: %an <%ae>%nCommitter: %cn <%ce>%n%n%B' \
        -- . ':(exclude)tools/publish/secret-scan.sh' ':(exclude)tools/publish/scan-allow.txt' \
      | grep -v -E '^(Author|Committer): Paul Schuldesz <paulschuldesz@gmail\.com>$' \
      | grep -v -E '^[ +-]?(From|Signed-off-by|Co-developed-by): Paul Schuldesz <paulschuldesz@gmail\.com>$' \
      > "$hits_file.log"
    grep_both -n "$hits_file.log" | filter_allowed > "$hits_file" || true
    rm -f "$hits_file.log"
    ;;
esac

if [ -s "$hits_file" ]; then
  echo "secret-scan: $(wc -l < "$hits_file") hit(s):"
  cut -c1-240 "$hits_file"
  exit 1
fi
echo "secret-scan: 0 hits ($mode: $target)"
