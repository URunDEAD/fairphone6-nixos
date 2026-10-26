#!/usr/bin/env bash
# Fetch the pinned upstream sources this tree builds against.
# Nothing here is vendored in git; every source is pinned by revision/hash.
#
#   tools/fetch-sources.sh [--kernel-from DIR] [--skip-kernel]
#
#   nixos-fp6/upstream/mobile-nixos   mobile-nixos @ MOBILE_NIXOS_REV + our two
#                                      patches (nixos-fp6/mobile-nixos-*.patch)
#   nixos-fp6/upstream/rmtfs           linux-msm/rmtfs @ RMTFS_REV
#   kernel-build/linux-<rev>.tar.gz    git archive of milos-linux @ KERNEL_REV
#   kernel-build/gmu-timeout/linux-<rev>.tar.gz   (same file, hard link)
#
# --kernel-from DIR uses an existing milos-linux checkout instead of cloning
# (about 250 MB shallow). The kernel recipes assert the archive's sha256, so a
# git with a different archive/gzip output fails loudly instead of silently
# building something else.
#
# The proprietary/device-derived inputs in nixos-fp6/assets/ are NOT fetched;
# see nixos-fp6/assets/README.md.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)

MOBILE_NIXOS_URL=https://github.com/mobile-nixos/mobile-nixos.git
MOBILE_NIXOS_REV=2c132754323fc1915e8d21dcfc0ef68ab084c6fb
RMTFS_URL=https://github.com/linux-msm/rmtfs.git
RMTFS_REV=b30a3eb38f9af283f18dbd3c7755653efc52c094
KERNEL_URL=https://forgejo.catcrafts.net/Catcrafts/milos-linux.git
KERNEL_REV=63289f4411520a2acb38bd533437446d33fbce74
KERNEL_SHA256=1a1aba170899273cbb97bce2dc0713957340a7be2e2d187faab6f821d60687ba

kernel_from=
skip_kernel=0
while [ $# -gt 0 ]; do
  case "$1" in
    --kernel-from) kernel_from=$2; shift 2 ;;
    --skip-kernel) skip_kernel=1; shift ;;
    -h|--help) sed -n '2,21p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

fetch_rev() { # url rev dir
  local url=$1 rev=$2 dir=$3
  if [ -d "$dir/.git" ] && [ "$(git -C "$dir" rev-parse HEAD)" = "$rev" ]; then
    echo "ok: $dir @ ${rev:0:12}"; return
  fi
  rm -rf "$dir"; mkdir -p "$dir"
  git -C "$dir" init -q
  git -C "$dir" remote add origin "$url"
  git -C "$dir" fetch -q --depth 1 origin "$rev"
  git -C "$dir" checkout -q --detach FETCH_HEAD
  echo "fetched: $dir @ ${rev:0:12}"
}

up="$root/nixos-fp6/upstream"
mkdir -p "$up"

fetch_rev "$MOBILE_NIXOS_URL" "$MOBILE_NIXOS_REV" "$up/mobile-nixos"
if ! git -C "$up/mobile-nixos" diff --quiet; then
  echo "ok: mobile-nixos patches already applied"
else
  for p in "$root"/nixos-fp6/mobile-nixos-zstd.patch "$root"/nixos-fp6/mobile-nixos-ffs-module.patch; do
    git -C "$up/mobile-nixos" apply "$p"
    echo "applied: $(basename "$p")"
  done
fi

fetch_rev "$RMTFS_URL" "$RMTFS_REV" "$up/rmtfs"

if [ "$skip_kernel" = 0 ]; then
  tarball="$root/kernel-build/linux-$KERNEL_REV.tar.gz"
  if [ -f "$tarball" ] && echo "$KERNEL_SHA256  $tarball" | sha256sum -c --quiet 2>/dev/null; then
    echo "ok: $tarball"
  else
    src=$kernel_from
    if [ -z "$src" ]; then
      src="$root/kernel-build/source"
      fetch_rev "$KERNEL_URL" "$KERNEL_REV" "$src"
    fi
    git -C "$src" archive --format=tar --prefix="linux-$KERNEL_REV/" "$KERNEL_REV" | gzip -n > "$tarball"
    echo "$KERNEL_SHA256  $tarball" | sha256sum -c --quiet || {
      echo "WARNING: archive hash differs from the pinned one (git/gzip version?)." >&2
      echo "         The kernel recipes will refuse it; see kernel-build/README.txt." >&2
    }
  fi
  # A symlink would change the Nix store path of the source: hard link (or copy).
  rm -f "$root/kernel-build/gmu-timeout/linux-$KERNEL_REV.tar.gz"
  ln "$tarball" "$root/kernel-build/gmu-timeout/linux-$KERNEL_REV.tar.gz" 2>/dev/null \
    || cp "$tarball" "$root/kernel-build/gmu-timeout/linux-$KERNEL_REV.tar.gz"
fi

missing=0
for a in fp6-firmware.tar fp6-kernel.tar boot-template.img; do
  [ -e "$root/nixos-fp6/assets/$a" ] || { echo "missing asset: nixos-fp6/assets/$a"; missing=1; }
done
[ "$missing" = 0 ] || echo "see nixos-fp6/assets/README.md for how to produce the assets from your own phone"
