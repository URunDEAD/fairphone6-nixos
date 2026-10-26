# Preservation policy layered on the exact reviewed transport package.
{ callPackage }:
(callPackage ./fingerprintd-transport-package.nix {}).overrideAttrs (old: {
  patches = (old.patches or []) ++ [ ./fingerprintd-enrollment-preservation.patch ];
})
