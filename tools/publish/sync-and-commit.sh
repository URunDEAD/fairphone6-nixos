#!/usr/bin/env bash
# One fix = one commit, in both repositories.
#
#   tools/publish/sync-and-commit.sh -m "subject" [-b "body"] PATH...
#   tools/publish/sync-and-commit.sh -m "subject" --all
#   tools/publish/sync-and-commit.sh --public-only -m "subject"
#   tools/publish/sync-and-commit.sh --status
#
# (a) commits PATH... (or --all changes) in the PRIVATE live workspace repo
#     ($FP6_WORKSPACE, default ~/Desktop/and) with the given message;
# (b) runs export-public.sh into the PUBLIC repo ($FP6_PUBLIC, default
#     ~/Desktop/fp6-nixos), which ends with the secret scan, and commits the
#     exported changes there with the same message; then scans the public
#     repo's whole history;
# (c) prints the upstream branches (CONTRIBUTING-UPSTREAM.md) that mention
#     the files you changed, so you can update them.
#
# Nothing is pushed. Every commit is authored as the configured git user and
# ends with the Co-Authored-By trailer below (set FP6_TRAILER= to drop it).
#
# Private inputs (never committed anywhere):
#   FP6_SECRET_PATTERNS  literal secret regexes for secret-scan.sh
#   FP6_EXPORT_DENY      extra '!' deny rules for export-public.sh
set -euo pipefail

workspace=${FP6_WORKSPACE:-$HOME/Desktop/and}
public=${FP6_PUBLIC:-$HOME/Desktop/fp6-nixos}
private_dir=$workspace/results/feature-push-20261024/publish
export FP6_SECRET_PATTERNS=${FP6_SECRET_PATTERNS:-$private_dir/secret-patterns.txt}
export FP6_EXPORT_DENY=${FP6_EXPORT_DENY:-$private_dir/export-deny.txt}
export FP6_WORKSPACE=$workspace
trailer=${FP6_TRAILER-Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>}
tools=$public/tools/publish

subject= body= all=0 public_only=0 status=0
paths=()
while [ $# -gt 0 ]; do
  case "$1" in
    -m) subject=$2; shift 2 ;;
    -b) body=$2; shift 2 ;;
    --all) all=1; shift ;;
    --public-only) public_only=1; shift ;;
    --status) status=1; shift ;;
    -h|--help) sed -n '2,26p' "$0"; exit 0 ;;
    --) shift; paths+=("$@"); break ;;
    *) paths+=("$1"); shift ;;
  esac
done

if [ "$status" = 1 ]; then
  echo "== private ($workspace)"; git -C "$workspace" status --short
  echo "== public  ($public)";    git -C "$public" status --short
  exit 0
fi
[ -n "$subject" ] || { echo "need -m \"subject\"" >&2; exit 2; }

msgfile=$(mktemp); trap 'rm -f "$msgfile"' EXIT
{ printf '%s\n' "$subject"
  [ -z "$body" ] || printf '\n%s\n' "$body"
  [ -z "$trailer" ] || printf '\n%s\n' "$trailer"; } > "$msgfile"

changed=()
# ---- (a) private commit ----
if [ "$public_only" = 0 ]; then
  if [ "$all" = 1 ]; then
    git -C "$workspace" add -A
  elif [ ${#paths[@]} -gt 0 ]; then
    git -C "$workspace" add -A -- "${paths[@]}"
  else
    echo "name the files of this one fix (or use --all):" >&2
    git -C "$workspace" status --short >&2; exit 2
  fi
  mapfile -t changed < <(git -C "$workspace" diff --cached --name-only)
  if [ ${#changed[@]} -eq 0 ]; then
    echo "private: nothing staged"
  else
    git -C "$workspace" commit -q -F "$msgfile"
    echo "private: $(git -C "$workspace" log --oneline -1)"
  fi
fi

# ---- (b) export + scan + public commit ----
"$tools/export-public.sh" --src "$workspace" --dst "$public"
git -C "$public" add -A
if git -C "$public" diff --cached --quiet; then
  echo "public: no exported changes"
else
  git -C "$public" diff --cached --stat | tail -1
  git -C "$public" commit -q -F "$msgfile"
  echo "public: $(git -C "$public" log --oneline -1)"
fi
"$tools/secret-scan.sh" --history "$public"

# ---- (c) upstream reminder ----
if [ ${#changed[@]} -gt 0 ] && [ -r "$public/CONTRIBUTING-UPSTREAM.md" ]; then
  shown=0
  for f in "${changed[@]}"; do
    b=$(basename "$f")
    rows=$(grep -F -- "$b" "$public/CONTRIBUTING-UPSTREAM.md" || true)
    if [ -n "$rows" ]; then
      [ "$shown" = 1 ] || echo "== upstream branches to update (see CONTRIBUTING-UPSTREAM.md):"
      shown=1
      echo "-- $f"; printf '%s\n' "$rows" | cut -c1-200
    fi
  done
  case "${changed[*]}" in
    *kernel-build/*) echo "-- kernel patch changed: regenerate fp6-upstream/linux/series with make-series.sh" ;;
  esac
fi
echo "nothing was pushed; see PUBLISHING.md"
