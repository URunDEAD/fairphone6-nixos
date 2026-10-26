# Separate selfie-only userspace extension; rear production package stays pinned.
{ callPackage }:
(callPackage ./camera-fp6-pmos-libcamera.nix {}).overrideAttrs (old: {
  pname = "libcamera-fp6-selfie";
  patches = (old.patches or []) ++ [ ./selfie-fp6-libcamera-helper.patch ];
  checkPhase = ''
    runHook preCheck
    meson test --no-rebuild --print-errorlogs fp6_sensor_helpers fp6_selfie_sensor_helper
    runHook postCheck
  '';
})
