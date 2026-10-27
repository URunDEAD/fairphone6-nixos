#!/usr/bin/env bash
# Regenerates ../lomiri-accessibility.patch from the full target's patched
# Lomiri tree WITHOUT this module (TREE: e.g. nix-build
# ../spread-close-all/test/patched-src.nix on a tree where the accessibility
# import is commented out, or the tree with ../lomiri-accessibility.patch
# reversed).
#   mkpatch.sh TREE > ../lomiri-accessibility.patch
set -eu
here=$(dirname "$(readlink -f "$0")")
tree=$(readlink -f "$1")
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
files="qml/ShellScreen.qml qml/Panel/Panel.qml qml/Panel/QuickSettings/QsBackend.qml"
for f in $files; do
  mkdir -p "$work/a/$(dirname "$f")" "$work/b/$(dirname "$f")"
  cp "$tree/$f" "$work/a/$f"; cp "$tree/$f" "$work/b/$f"
done
chmod -R u+w "$work"
python3 "$here/edit-lomiri.py" "$work/b" >&2
cd "$work"
for f in $files; do diff -u --label "a/$f" --label "b/$f" "a/$f" "b/$f" || true; done
