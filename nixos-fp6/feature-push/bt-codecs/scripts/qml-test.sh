#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
preview_env=${1:?Pass output of nix-build nix/preview.nix}
panel=$(sed -n 's/^[[:space:]]*export FP6_BT_PANEL=//p' "$preview_env/bin/fp6-bt-preview")
mkdir -p tests/qml/generated
cp --remove-destination "$panel/ProvidePinCodeDialog.qml" "$panel/ProvidePasskeyDialog.qml" "$panel/ConfirmPasskeyDialog.qml" tests/qml/generated/
exec "$preview_env/bin/fp6-bt-preview" --test -input tests/qml
