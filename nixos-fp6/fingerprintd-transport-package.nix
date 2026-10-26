# Diagnostic-only delta: preserve errno and expose transport errors separately
# from secure-world loader verdicts; failed daemon initialization exits nonzero.
{ callPackage }:
(callPackage ./fingerprintd-package.nix {}).overrideAttrs (old: {
  patches = (old.patches or []) ++ [ ./fingerprintd-transport-diagnostics.patch ];
})
