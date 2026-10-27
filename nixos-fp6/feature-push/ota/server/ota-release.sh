#!/usr/bin/env bash
# One-command OTA release from the current tree (laptop side).
#
#   nixos-fp6/feature-push/ota/server/ota-release.sh [options]
#
#   --target FILE        system to build (default nixos-fp6/feature-push/trial-lomiri-full-kernel6-fp6.nix)
#   --root DIR           release root served by fp6-release serve (default /srv/fp6-ota)
#   --keys DIR           release keys (default results/feature-push-20261024/ota/keys)
#   --changelog-file F   changelog text (default: git log since the last published release)
#   --kernel-notes TEXT  kernel notes for the manifest
#   --no-bump            publish the tree's release counter as it is (e.g. a first baseline)
#   --commit-bump        commit ota/release.nix after a successful publish (path-only)
#   --dry-run            show what would happen; build and publish nothing
#
# Steps:
#  1. Pick the release number: max(tree counter, published release) + 1 (or
#     the tree counter with --no-bump) and write it to ota/release.nix.
#  2. Build the toplevel and its boot image on the ARM builder.
#  3. Copy the closure to this machine (copy-from-builder).
#  4. Check the built system's /etc/fp6-release says that release.
#  5. fp6-release publish (signs the store paths and the manifest) + verify.
#  6. With --commit-bump: git commit --only ota/release.nix.
# On any failure before the publish, ota/release.nix is restored.
# Never touches the phone.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../../../.." && pwd)
ota="$repo/nixos-fp6/feature-push/ota"
target="$repo/nixos-fp6/feature-push/trial-lomiri-full-kernel6-fp6.nix"
root=/srv/fp6-ota
keys="$repo/results/feature-push-20261024/ota/keys"
changelog_file=""
kernel_notes=""
bump=1
commit=0
dry=0
while [ $# -gt 0 ]; do
  case "$1" in
    --target) target=$(realpath "$2"); shift 2 ;;
    --root) root="$2"; shift 2 ;;
    --keys) keys="$2"; shift 2 ;;
    --changelog-file) changelog_file=$(realpath "$2"); shift 2 ;;
    --kernel-notes) kernel_notes="$2"; shift 2 ;;
    --no-bump) bump=0; shift ;;
    --commit-bump) commit=1; shift ;;
    --dry-run) dry=1; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done

say() { printf '\n== %s\n' "$*"; }
relnix="$ota/release.nix"
tree_rel=$(sed -n 's/^ *release = \([0-9]\+\);.*/\1/p' "$relnix")
channel=$(sed -n 's/^ *channel = "\([^"]*\)";.*/\1/p' "$relnix")
[ -n "$tree_rel" ] && [ -n "$channel" ] || { echo "cannot read $relnix" >&2; exit 1; }
manifest="$root/channels/$channel/manifest.json"
pub_rel=0
if [ -f "$manifest" ]; then
  pub_rel=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["release"])' "$manifest")
fi
if [ $bump = 1 ]; then
  new=$(( (tree_rel > pub_rel ? tree_rel : pub_rel) + 1 ))
else
  new=$tree_rel
  [ "$new" -gt "$pub_rel" ] || { echo "tree release $new is not newer than the published $pub_rel (drop --no-bump)" >&2; exit 1; }
fi
for f in manifest.key nix-cache.secret nix-cache.pub; do
  [ -f "$keys/$f" ] || { echo "missing $keys/$f (fp6-release keygen)" >&2; exit 1; }
done
[ -d "$root" ] && [ -w "$root" ] || { echo "$root must exist and be writable by $(id -un) (see ota/server/fp6-ota-server.nix)" >&2; exit 1; }

say "release $new on channel $channel (tree $tree_rel, published $pub_rel), target $(basename "$target")"
if [ $dry = 1 ]; then
  echo "dry run: would set release = $new, build, copy, publish into $root"
  exit 0
fi

orig=$(cat "$relnix")
restore() { if [ "${published:-0}" = 0 ]; then printf '%s\n' "$orig" > "$relnix"; echo "restored $relnix" >&2; fi; }
trap restore ERR INT TERM
sed -i "s/^\( *release = \)[0-9]\+;/\1$new;/" "$relnix"

say "build on the ARM builder"
paths=$("$repo/.macmini-builder/nix-remote" build --eval-store daemon --file "$target" \
  config.system.build.toplevel outputs.android.android-bootimg --no-link --print-out-paths)
top=$(printf '%s\n' "$paths" | sed -n 1p)
img=$(printf '%s\n' "$paths" | sed -n 2p)
echo "toplevel   $top"
echo "boot image $img"

say "copy the closure to this machine"
"$repo/.macmini-builder/copy-from-builder" "$top" "$img"
got=$(python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["release"])' "$top/etc/fp6-release")
[ "$got" = "$new" ] || { echo "built system says release $got, expected $new" >&2; false; }

say "fp6-release"
rel=$(nix-build --no-out-link -E "(import (import $repo/nixos-fp6/upstream/mobile-nixos/npins).nixpkgs {}).callPackage $ota/package.nix {}" -A core)/bin/fp6-release

if [ -z "$changelog_file" ]; then
  changelog_file=$(mktemp)
  since=""
  [ -f "$root/channels/$channel/releases/$pub_rel/git-commit" ] && since=$(cat "$root/channels/$channel/releases/$pub_rel/git-commit")
  if [ -n "$since" ] && git -C "$repo" cat-file -e "$since" 2>/dev/null; then
    git -C "$repo" log --no-merges --format='- %s' "$since..HEAD" -- nixos-fp6 kernel-build > "$changelog_file"
  else
    git -C "$repo" log --no-merges --format='- %s' -15 -- nixos-fp6 kernel-build > "$changelog_file"
  fi
  [ -s "$changelog_file" ] || echo "- Rebuild of $(git -C "$repo" rev-parse --short HEAD)" > "$changelog_file"
fi

say "publish"
"$rel" publish --root "$root" --channel "$channel" --toplevel "$top" --boot-image "$img" --keys "$keys" \
  --changelog-file "$changelog_file" ${kernel_notes:+--kernel-notes "$kernel_notes"}
published=1
git -C "$repo" rev-parse HEAD > "$root/channels/$channel/releases/$new/git-commit"
"$rel" verify --root "$root" --channel "$channel" --pubkey "$keys/manifest.pub"
trap - ERR INT TERM

if [ $commit = 1 ]; then
  say "commit ota/release.nix"
  git -C "$repo" commit --only -m "ota: release $new

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>" -- "$relnix"
else
  echo "ota/release.nix now says release = $new; commit it (path-only) so the next phone install carries it."
fi

say "done: release $new"
echo "On the phone: Settings > System update > Check now (server must point at this machine)."
