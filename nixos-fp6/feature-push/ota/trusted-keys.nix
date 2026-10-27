# PUBLIC keys the phone trusts for updates (the secret halves live only on
# the release machine: results/feature-push-20261024/ota/keys, git-ignored).
# Created 2026-10-27 with `fp6-release keygen`.
{
  # minisign Ed25519 key E69F7419E592A7F1: signs manifest.json.
  manifest = "RWTxp5LlGXSf5uIa0kbVeCcCJpPSFvG7lPuw3aVybdI/VwVsJTDPYBad";
  # Nix store signing key: every path in the update cache carries it.
  nix = "fp6-ota-1:k0bLcoyXAVrjMzk5wql1wvw8/pToXRTGMn6/OjMntrc=";
}
