#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
preview_env=${1:?Pass the output path from nix-build nix/preview.nix}
mkdir -p previews
for state in quality stable call disconnected error; do
  timeout 30 "$preview_env/bin/fp6-bt-preview" qml/Preview.qml --"$state" "--output=$PWD/previews/$state.png" > "previews/$state.log" 2>&1
  test -s "previews/$state.png"
done
panel=$(sed -n 's/^[[:space:]]*export FP6_BT_PANEL=//p' "$preview_env/bin/fp6-bt-preview")
mkdir -p tests/qml/generated
cp --remove-destination "$panel/ConfirmPasskeyDialog.qml" tests/qml/generated/
timeout 30 "$preview_env/bin/fp6-bt-preview" qml/PairingPreview.qml "--output=$PWD/previews/pairing-confirmation.png" > previews/pairing-confirmation.log 2>&1
test -s previews/pairing-confirmation.png
