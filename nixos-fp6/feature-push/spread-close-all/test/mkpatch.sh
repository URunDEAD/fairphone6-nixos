#!/usr/bin/env bash
# Regenerates ../lomiri-spread-close-all.patch from the full target's patched
# Lomiri tree (TREE: unpacked + every other module's patches, e.g. from
# test/patched-src.nix).
#   mkpatch.sh TREE > ../lomiri-spread-close-all.patch
set -eu
here=$(dirname "$(readlink -f "$0")")
tree=$(readlink -f "$1")
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
files="qml/Components/FloatingFlickable.qml qml/Stage/Stage.qml qml/Shell.qml"
for f in $files; do
  mkdir -p "$work/a/$(dirname "$f")" "$work/b/$(dirname "$f")"
  cp "$tree/$f" "$work/a/$f"; cp "$tree/$f" "$work/b/$f"
done
chmod -R u+w "$work"
python3 "$here/edit-spread-close-all.py" "$work/b" >&2
cd "$work"
for f in $files; do diff -u --label "a/$f" --label "b/$f" "a/$f" "b/$f" || true; done
