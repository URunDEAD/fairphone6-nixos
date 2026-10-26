# Front camera (Samsung S5KKD1) Bayer order fixed at the source.
#
# The fp6-s5kkd1 driver starts streaming with the vendor's exact write
# 0x0100 = 0x0103 (stream on + H and V flip) and reported
# MEDIA_BUS_FMT_SGRBG10_1X10, with the note "Vendor CFA numeric 1; GRBG
# interpretation requires physical color check". Qualcomm's sensor XML
# colorFilterArrangement enumerates BGGR=0, GBRG=1, GRBG=2, RGGB=3, so the
# vendor's 1 is GBRG, which is GRBG with R and B exchanged (both flips move
# the 2x2 pattern by one row and one column). The physical check agreed:
# selfies had blue skin until R/B were swapped (swap-rb workaround).
#
# This rebuilds the same pinned 0.3 diagnostic driver source for the
# running kernel with only the media bus code changed to SGBRG10, keeps its
# MODULE_VERSION (the boot-load unit checks it) and turns the swap-rb
# relabel off. The boot-load unit verifies the module's sha256 from the
# target's kernelPin.selfieModuleSha256, which must be updated to this
# module's hash (passthru.sha256Note documents how).
{ config, lib, pkgs, ... }:
let
  kernel = config.mobile.boot.stage-1.kernel.package;
  base = pkgs.callPackage ../../selfie-fp6-diagnostic-driver-kernel4.nix {
    inherit kernel;
    expectedKernelDrvPath = kernel.drvPath;
    kernelRelease = kernel.modDirVersion;
  };
  driver = base.overrideAttrs (old: {
    __intentionallyOverridingVersion = true;
    version = old.version + "-gbrg";
    postPatch = (old.postPatch or "") + ''
      substituteInPlace fp6-s5kkd1.c \
        --replace-fail 'Vendor CFA numeric1; GRBG interpretation requires physical color check.' \
                       'Vendor CFA numeric 1 = GBRG (Qualcomm enum); confirmed by colour check.' \
        --replace-fail MEDIA_BUS_FMT_SGRBG10_1X10 MEDIA_BUS_FMT_SGBRG10_1X10
      ! grep -q SGRBG10 fp6-s5kkd1.c
      test "$(grep -c MEDIA_BUS_FMT_SGBRG10_1X10 fp6-s5kkd1.c)" = 4
    '';
    passthru = old.passthru // {
      bayerOrder = "GBRG";
      sha256Note = "sha256sum <out>/share/fp6-selfie-diagnostic/fp6-s5kkd1.ko -> kernelPin.selfieModuleSha256";
    };
  });
in {
  system.build.fp6-selfie-kernel4-driver = lib.mkForce driver;
  system.build.fp6-selfie-gbrg-driver = driver;
  hardware.fp6LomiriCamera.frontBayerFixed = true;
}
