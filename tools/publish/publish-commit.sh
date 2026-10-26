#!/usr/bin/env bash
# Publish already-committed private fixes, one public commit per private commit.
#
#   tools/publish/publish-commit.sh SHA...
#
# For each SHA (in the private live repo $FP6_WORKSPACE, default ~/Desktop/and):
# copy the files that commit touched, at that commit's content, into the public
# repo ($FP6_PUBLIC, default ~/Desktop/fp6-nixos); skip files that do not pass
# export-public's allow/deny rules; secret-scan exactly those files; commit with
# the same message. Uncommitted work in progress in the live tree is never
# copied. Nothing is pushed.
set -euo pipefail
workspace=${FP6_WORKSPACE:-$HOME/Desktop/and}
public=${FP6_PUBLIC:-$HOME/Desktop/fp6-nixos}
tools="$public/tools/publish"
allow="$tools/allowlist.txt"
[ "$#" -gt 0 ] || { echo "usage: $0 SHA..." >&2; exit 2; }

selected() {  # $1 = path; true if the allowlist selects it and no deny rule matches
  local f=$1 ok=1 line neg pat
  while IFS= read -r line; do
    case "$line" in ''|'#'*) continue ;; esac
    neg=0; pat=$line
    [ "${line#!}" != "$line" ] && { neg=1; pat=${line#!}; }
    # shellcheck disable=SC2053
    if [[ $f == $pat ]]; then [ $neg = 1 ] && return 1; ok=0; fi
  done < <(cat "$allow" "${FP6_EXPORT_DENY:-/dev/null}" 2>/dev/null)
  return $ok
}

for sha in "$@"; do
  mapfile -t files < <(git -C "$workspace" show --name-only --format= "$sha")
  staged=$(mktemp)
  for f in "${files[@]}"; do
    [ -n "$f" ] || continue
    selected "$f" || { echo "skip (not public): $f"; continue; }
    if git -C "$workspace" cat-file -e "$sha:$f" 2>/dev/null; then
      mkdir -p "$public/$(dirname "$f")"
      git -C "$workspace" show "$sha:$f" > "$public/$f"
    elif [ -e "$public/$f" ]; then
      git -C "$public" rm -q -- "$f"
    fi
    git -C "$public" add -A -- "$f" 2>/dev/null || true
    [ -e "$public/$f" ] && echo "$public/$f" >> "$staged"
  done
  if [ -s "$staged" ] && ! "$tools/secret-scan.sh" --files "$staged"; then
    git -C "$public" reset -q
    rm -f "$staged"
    echo "$sha: secret scan hit; not committed" >&2
    exit 1
  fi
  rm -f "$staged"
  if git -C "$public" diff --cached --quiet; then
    echo "$sha: nothing public to commit"
  else
    git -C "$public" commit -q -m "$(git -C "$workspace" log -1 --format=%B "$sha")"
    echo "$sha -> $(git -C "$public" log --oneline -1)"
  fi
done
"$tools/secret-scan.sh" --history "$public"
