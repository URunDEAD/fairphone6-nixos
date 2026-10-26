#!/usr/bin/env bash
# Export the curated, public part of the private FP6 workspace into the
# public repository.
#
# Usage:
#   tools/publish/export-public.sh [--src LIVE_WORKSPACE] [--dst PUBLIC_REPO] [--dry-run]
#
# Defaults: --src $FP6_WORKSPACE (or ~/Desktop/and), --dst the repo this
# script lives in.
#
# What it does:
#   1. selects files from LIVE_WORKSPACE with tools/publish/allowlist.txt
#      (globs; '!' lines deny and always win)
#   2. refuses anything that is not small text (binary or > 1 MiB)
#   3. copies them into PUBLIC_REPO at the same relative path, applies
#      tools/publish/sanitize.sed, and removes files exported earlier that
#      are no longer selected (tracked in tools/publish/export-manifest.txt)
#   4. reports files that the build targets (targets.txt) import but the
#      allowlist misses
#   5. runs tools/publish/secret-scan.sh on the exported tree; exit 1 on hits
#
# It never commits; tools/publish/sync-and-commit.sh does that.
# Public-only files (README.md, docs/, tools/publish/, tools/phone-ssh, ...)
# are never listed in the manifest, so the export never touches them.
set -euo pipefail
shopt -s globstar dotglob nullglob

here=$(cd "$(dirname "$0")" && pwd)
dst=$(cd "$here/../.." && pwd)
src=${FP6_WORKSPACE:-$HOME/Desktop/and}
dry=0
while [ $# -gt 0 ]; do
  case "$1" in
    --src) src=$2; shift 2 ;;
    --dst) dst=$2; shift 2 ;;
    --dry-run) dry=1; shift ;;
    -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
src=$(cd "$src" && pwd)
allow="$here/allowlist.txt"
manifest="$here/export-manifest.txt"
max_bytes=$((1024 * 1024))

[ -d "$src/nixos-fp6" ] || { echo "export: $src does not look like the FP6 workspace" >&2; exit 2; }

# ---- 1. select ----
declare -A selected=()
# Extra deny rules from a private, untracked file (topics that must never be
# exported are named there, not here): $FP6_EXPORT_DENY.
deny_extra=${FP6_EXPORT_DENY:-$HOME/.config/fp6-publish/export-deny.txt}
mapfile -t rules < <(grep -v -E '^[[:space:]]*(#|$)' "$allow"; \
  if [ -r "$deny_extra" ]; then grep -E '^!' "$deny_extra"; else echo "export: note: no private deny file at $deny_extra" >&2; fi)
cd "$src"
for rule in "${rules[@]}"; do
  case "$rule" in '!'*) continue ;; esac
  for f in $rule; do
    [ -f "$f" ] && [ ! -L "$f" ] && selected["$f"]=1
  done
done
for rule in "${rules[@]}"; do
  case "$rule" in
    '!'*)
      pat=${rule#!}
      for f in "${!selected[@]}"; do
        # shellcheck disable=SC2053
        [[ $f == $pat ]] && unset 'selected[$f]'
      done ;;
  esac
done

# ---- 2. text-only, size limit ----
rejected=()
for f in "${!selected[@]}"; do
  size=$(stat -c %s "$f")
  enc=$(file -b --mime-encoding "$f")
  if [ "$size" -gt "$max_bytes" ] || { [ "$enc" != us-ascii ] && [ "$enc" != utf-8 ]; }; then
    rejected+=("$f ($enc, $size bytes)")
    unset 'selected[$f]'
  fi
done
mapfile -t files < <(printf '%s\n' "${!selected[@]}" | LC_ALL=C sort)
echo "export: ${#files[@]} files selected from $src"
for r in "${rejected[@]+"${rejected[@]}"}"; do echo "export: skipped non-text/large: $r"; done

# ---- 4 (early). closure coverage report ----
missing=()
if [ -r "$here/targets.txt" ] && command -v python3 >/dev/null; then
  mapfile -t targets < <(grep -v -E '^[[:space:]]*(#|$)' "$here/targets.txt")
  while read -r kind path _; do
    [ "$kind" = F ] || continue
    case "$path" in nixos-fp6/upstream/*|kernel-build/source/*) continue ;; esac
    case "$path" in *.tar|*.tar.gz|*.img|*.bin) continue ;; esac   # user-supplied assets
    [ -n "${selected[$path]+x}" ] || missing+=("$path")
  done < <(FP6_CLOSURE_ROOT="$src" python3 "$here/closure.py" "${targets[@]}" 2>/dev/null || true)
fi
for m in "${missing[@]+"${missing[@]}"}"; do echo "export: WARNING: imported by a target but not exported: $m"; done

if [ "$dry" = 1 ]; then printf '%s\n' "${files[@]}"; exit 0; fi

# ---- 3. copy + sanitize + prune ----
for f in "${files[@]}"; do
  mkdir -p "$dst/$(dirname "$f")"
  sed -E -f "$here/sanitize.sed" "$f" > "$dst/$f.tmp-export"
  chmod --reference="$f" "$dst/$f.tmp-export"
  mv "$dst/$f.tmp-export" "$dst/$f"
done
if [ -r "$manifest" ]; then
  while IFS= read -r old; do
    [ -n "$old" ] || continue
    if [ -z "${selected[$old]+x}" ]; then
      rm -f "$dst/$old" && echo "export: removed (no longer selected): $old"
      rmdir -p --ignore-fail-on-non-empty "$dst/$(dirname "$old")" 2>/dev/null || true
    fi
  done < "$manifest"
fi
printf '%s\n' "${files[@]}" > "$manifest"

# ---- 5. secret scan ----
"$here/secret-scan.sh" "$dst"
