# Qt5 QtMultimedia with GStreamer photography enabled (flash/focus controls in
# the camerabin backend; stock nixpkgs builds it without gst-plugins-bad) and
# the FP6 camerabin patch: FlashVideoLight/FlashTorch <-> GstPhotography
# FILL_IN, realtime x264enc settings (kbit/s bitrate, ultrafast, zerolatency).
# qtmultimedia-camerabin-gstinit-fp6.patch: the camerabin service plugin looked
# up QT_GSTREAMER_CAMERABIN_SRC before gst_init, so QCameraInfo::defaultCamera()
# fell back to /dev/video* and QCamera reported ServiceMissingError ("Cannot
# access camera ... permission") in lomiri-camera-app.
# Only used by the FP6 lomiri-camera-app override, never as a global overlay.
{ libsForQt5, gst_all_1, binutils }:
libsForQt5.qtmultimedia.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [ ./qtmultimedia-camerabin-fp6.patch ./qtmultimedia-camerabin-gstinit-fp6.patch ];
  buildInputs = (old.buildInputs or [ ]) ++ [ gst_all_1.gst-plugins-bad ];
  env = (old.env or { }) // {
    NIX_CFLAGS_COMPILE = "-I${gst_all_1.gst-plugins-bad.dev}/include/gstreamer-1.0";
  };
  nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ binutils ];
  # Fail the build unless the camerabin plugin really links GstPhotography.
  postFixup = (old.postFixup or "") + ''
    p=$(find "$bin" -name libgstcamerabin.so | head -n1)
    test -n "$p"
    nm -D "$p" | grep -q gst_photography_set_flash_mode
  '';
})
