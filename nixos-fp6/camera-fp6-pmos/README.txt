Original FP6 libcamera downstream patch series and main-sensor tuning.
Source: https://forgejo.catcrafts.net/Catcrafts/fp6-img
Commit: f98289ce013c2945cd986d76ea8ca6a7517ba914
Path: aports/temp/libcamera
Original package: 99990.7.2-r10. Base upstream v0.7.2 (191e202178f02430b5942397c70d215cdd2056fa).
All16 patches apply in order without modifications to that exact base.
APKBUILD is preserved for provenance; the Nix recipe does not execute it.
Original comments, authorship and tuning-data provenance remain intact.
Tuning comments attribute color calibration to numeric stock-vendor-blob data.
Only IMX896-specific tuning is copied; other-board files in the APK are omitted.
This series includes WIP autofocus: LensPosition uses percent travel, not the
standard dioptres. Image quality, lens safety/range and app capture remain
unvalidated on this NixOS image. No camera is activated by building this recipe.
