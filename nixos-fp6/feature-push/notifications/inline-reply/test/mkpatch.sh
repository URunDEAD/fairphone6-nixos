#!/usr/bin/env bash
# Regenerates ../lomiri-inline-reply.patch from the full target's patched
# Lomiri tree (TREE: unpacked + every other module's patches and postPatch,
# without this one), e.g. from test/patched-src.nix with this module off:
#   nix-build test/patched-src.nix -o $SCRATCH/psrc
#   test/mkpatch.sh $SCRATCH/psrc > lomiri-inline-reply.patch
set -eu
here=$(dirname "$(readlink -f "$0")")
tree=$(readlink -f "$1")
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
files="qml/Notifications/Notification.qml qml/Notifications/Notifications.qml qml/Shell.qml
       qml/Panel/Panel.qml qml/Panel/QuickSettings/ControlCentre.qml qml/Panel/QuickSettings/QsNotifications.qml"
for f in $files; do
  mkdir -p "$work/a/$(dirname "$f")" "$work/b/$(dirname "$f")"
  cp "$tree/$f" "$work/a/$f"; cp "$tree/$f" "$work/b/$f"
done
chmod -R u+w "$work"
python3 "$here/edit-inline-reply.py" "$work/b" >&2
cd "$work"
for f in $files; do diff -u --label "a/$f" --label "b/$f" "a/$f" "b/$f" || true; done
