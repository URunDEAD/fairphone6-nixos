# Isolated successor: preserve the two exact4 CAMSS fixes and align Milos RDI
# lines to its documented 128-bit bus width. No sensor/register recipe changes.
{ lib, callPackage, kernel
, expectedKernelDrvPath ? "/nix/store/lxg2kmffvq9xlac525byx12baqnqll3h-linux-fp6-nixos-7.2.0.drv"
, kernelRelease ? "7.2.0-fp6-nixos4"
}:
assert builtins.hashFile "sha256" ./selfie-camss-kernel4/milos-rdi-align-16.patch
  == "8eeb09d87314c4d71ef5f35a1f442723b2c1417acbfc9e9837e79b03daa29a1f";
(callPackage ./selfie-camss-kernel4.nix { inherit kernel expectedKernelDrvPath kernelRelease; }).overrideAttrs (old: {
  pname = "fp6-camss-kernel4-rdi16";
  version = "1";
  patches = old.patches ++ [ ./selfie-camss-kernel4/milos-rdi-align-16.patch ];
  meta = old.meta // {
    description = "Exact kernel4 CAMSS with DPHY, PHY3 selection and 16-byte Milos RDI alignment";
  };
})
