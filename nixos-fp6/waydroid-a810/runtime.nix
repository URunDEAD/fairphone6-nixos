# Runtime subset of the pinned source build. Keep its compiler/NDK audit JSON
# out of the phone closure; all seven libraries and their content pins remain.
{ lib, runCommand, python3, binutils, sourcePackage }:
runCommand "fp6-waydroid-mesa-a810-runtime-26.1.4" {
  nativeBuildInputs = [ python3 binutils ];
  passthru = {
    inherit sourcePackage;
    isBinaryRepack = false;
    hardwareAcceptance = false;
  };
  meta = {
    description = "Seven-library Android A810 runtime overlay from pinned Mesa source";
    license = with lib.licenses; [ mit bsd2 asl20 ];
    platforms = [ "aarch64-linux" ];
  };
} ''
  mkdir -p $out/share/fp6-waydroid-a810
  cp -r ${sourcePackage}/lib64 $out/
  python3 ${./validate-overlay.py} $out/lib64 \
    --expected ${sourcePackage}/share/fp6-waydroid-a810/artifacts.json \
    --consumer-imports ${./consumer-imports.json} \
    --output $out/share/fp6-waydroid-a810/artifacts.json
  cp ${sourcePackage}/share/fp6-waydroid-a810/provenance.json $out/share/fp6-waydroid-a810/
  cp ${sourcePackage}/share/fp6-waydroid-a810/license.rst $out/share/fp6-waydroid-a810/
''
