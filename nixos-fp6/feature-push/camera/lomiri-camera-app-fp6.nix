# lomiri-camera-app wired to the FP6 libcamera stack.
#   Qt5 camerabin -> fp6camerabinsrc (wrappercamerabinsrc subclass, primary =
#   back IMX896, secondary = front S5KKD1, mount angles, GstPhotography flash)
#   -> fp6libcamerasrc (libcamerasrc + softISP, fixed stream size) .
# Recording: camerabin encodebin, MP4 H.264 (+AAC via fdkaacenc).
# hardwareEncode=false hides the Iris V4L2 m2m encoder (v4l2h264enc) so
# encodebin picks x264enc; set true only after camera-test.sh step "venc" passes.
{ lib, lomiri, libsForQt5, gst_all_1, callPackage
, libcamera                      # MUST be the phone's hardware.fp6Camera.package
, hardwareEncode ? false
, configFile ? "/etc/fp6-camera/camera.conf"
}:
let
  qtmultimediaFp6 = callPackage ./qtmultimedia-fp6.nix { };
  gstFp6Camera = callPackage ./gst-fp6camera.nix { };
  hwEncoders = [ "v4l2h264enc" "v4l2h265enc" "v4l2vp8enc" "v4l2vp9enc" ];
  rankOverride = lib.concatMapStringsSep "," (e: "${e}:NONE") hwEncoders;
in
(lomiri.lomiri-camera-app.override {
  qtmultimedia = qtmultimediaFp6;
  qzxing = libsForQt5.qzxing.override { qtmultimedia = qtmultimediaFp6; };
}).overrideAttrs (old: {
  pname = "lomiri-camera-app";   # keep the name: desktop/app-id lookups
  # init-mode: CameraApplication::m_mode was never initialised (fixed upstream
  # after 4.1.1); garbage == BARCODE_READER loaded barcode-reader-app.qml,
  # i.e. no controls and a permanent "Scanning..." overlay.
  patches = (old.patches or [ ]) ++ [
    ./lomiri-camera-app-single-qguiapplication.patch
    ./lomiri-camera-app-init-mode.patch
    # wayland-device-mode: not "desktop mode" on Lomiri/Mir (QPA "wayland"),
    # otherwise the viewfinder is mirrored left-right.
    ./lomiri-camera-app-wayland-device-mode.patch
    # slideshow-exif: photo roll Images honour EXIF Orientation.
    ./lomiri-camera-app-slideshow-exif.patch
    # front-viewfinder: front frames are not HAL-mirrored here; rotate by the
    # sensor orientation and mirror only the preview.
    ./lomiri-camera-app-front-viewfinder.patch
  ];
  buildInputs = old.buildInputs ++ [ gstFp6Camera libcamera gst_all_1.gst-libav ];
  # Upstream QML/flake8 tests under xvfb are unrelated to the device path.
  doCheck = false;
  preFixup = (old.preFixup or "") + ''
    qtWrapperArgs+=(
      --set QT_GSTREAMER_CAMERABIN_SRC fp6camerabinsrc
      --set QT_GSTREAMER_CAMERABIN_VIDEOSRC fp6libcamerasrc
      --set-default FP6_CAMERA_CONFIG ${configFile}
      --prefix LIBCAMERA_IPA_CONFIG_PATH : ${gstFp6Camera}/share/libcamera/ipa
      --set QT_GSTREAMER_USE_OPENGL_PLUGIN 0
      ${lib.optionalString (!hardwareEncode) "--set GST_PLUGIN_FEATURE_RANK ${rankOverride}"}
    )
  '';
  # Refuse a wrapper that could load the stock (photography-less) camerabin.
  postFixup = (old.postFixup or "") + ''
    w=$out/bin/lomiri-camera-app
    grep -q fp6camerabinsrc "$w"
    grep -q ${qtmultimediaFp6.bin} "$w"
    grep -q ${gstFp6Camera}/lib/gstreamer-1.0 "$w"
    grep -q ${libcamera}/lib/gstreamer-1.0 "$w"
    if grep -o '/nix/store/[a-z0-9]*-qtmultimedia-[^/:]*' "$w" | sort -u | grep -v ${baseNameOf qtmultimediaFp6.bin} | grep -v ${baseNameOf qtmultimediaFp6.out} | grep -v ${baseNameOf qtmultimediaFp6.dev} | grep -q .; then
      echo "wrapper references a second qtmultimedia" >&2; exit 1
    fi
  '';
  passthru = (old.passthru or { }) // { inherit qtmultimediaFp6 gstFp6Camera; };
})
