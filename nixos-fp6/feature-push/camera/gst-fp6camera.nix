# GStreamer glue between Qt5 camerabin (lomiri-camera-app) and libcamera.
# Elements: fp6camerabinsrc (camera selection/orientation/flash) and
# fp6libcamerasrc (fixed-size libcamerasrc + scaler). Offline self test runs
# on the native builder with videotestsrc and a temp file as the "LED".
{ lib, stdenv, pkg-config, gst_all_1 }:
stdenv.mkDerivation {
  pname = "gst-fp6camera";
  version = "0.1";
  src = ./gst-fp6camera;
  strictDeps = true;
  nativeBuildInputs = [ pkg-config ];
  buildInputs = with gst_all_1; [ gstreamer gst-plugins-base gst-plugins-bad ];

  buildPhase = ''
    runHook preBuild
    flags="$(pkg-config --cflags gstreamer-1.0 gstreamer-photography-1.0 gstreamer-video-1.0 gstreamer-tag-1.0)"
    libs="$(pkg-config --libs gstreamer-1.0 gstreamer-photography-1.0 gstreamer-video-1.0 gstreamer-tag-1.0) -lgstbasecamerabinsrc-1.0 -lm"
    $CC -shared -fPIC -O2 -Wall \
      -DFP6CAM_CAMERABIN_PLUGIN='"${gst_all_1.gst-plugins-bad}/lib/gstreamer-1.0/libgstcamerabin.so"'  -Werror=implicit-function-declaration \
      -Werror=incompatible-pointer-types $flags -o libgstfp6camera.so fp6camera.c $libs
    $CC -O2 -Wall $flags $(pkg-config --cflags gstreamer-pbutils-1.0) -o fp6camera-selftest \
      fp6camera-selftest.c $libs $(pkg-config --libs gstreamer-pbutils-1.0)
    # manual controls / long exposure from the command line (live camera)
    $CC -O2 -Wall -DFP6_TUNING_DIR=\"$out/share/libcamera/ipa\" $(pkg-config --cflags gstreamer-1.0) -o fp6-long-exposure \
      fp6camera-manualtest.c $(pkg-config --libs gstreamer-1.0)
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    install -Dm755 libgstfp6camera.so $out/lib/gstreamer-1.0/libgstfp6camera.so
    install -Dm755 fp6camera-selftest $out/libexec/fp6camera-selftest
    install -Dm755 fp6-long-exposure $out/bin/fp6-long-exposure
    # softISP tuning for the sensors the libcamera package has none for;
    # used through LIBCAMERA_IPA_CONFIG_PATH=$out/share/libcamera/ipa
    install -Dm644 -t $out/share/libcamera/ipa/simple tuning/ov13b10.yaml tuning/fp6-s5kkd1.yaml
    runHook postInstall
  '';

  doInstallCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  installCheckPhase = ''
    runHook preInstallCheck
    export HOME=$TMPDIR GST_REGISTRY=$TMPDIR/registry.bin
    export GST_PLUGIN_SYSTEM_PATH_1_0="$out/lib/gstreamer-1.0:${lib.makeSearchPath "lib/gstreamer-1.0"
      (map lib.getLib (with gst_all_1; [ gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad ]))}"
    echo 0 > $TMPDIR/led
    cat > $TMPDIR/camera.conf <<EOF
    [flash]
    led-brightness-path=$TMPDIR/led
    brightness=160
    preflash-ms=200
    capture-hold-max-ms=800
    EOF
    export FP6_CAMERA_CONFIG=$TMPDIR/camera.conf FP6_SELFTEST_LED=$TMPDIR/led \
      FP6_SELFTEST_IMG=$TMPDIR/img.jpg
    ${lib.getBin gst_all_1.gstreamer}/bin/gst-inspect-1.0 fp6camerabinsrc | grep -q sensor-mount-angle
    ${lib.getBin gst_all_1.gstreamer}/bin/gst-inspect-1.0 fp6libcamerasrc | grep -q device
    ${lib.getBin gst_all_1.gstreamer}/bin/gst-inspect-1.0 fp6libcamerasrc | grep -q zoom
    # manual controls section
    for p in exposure-us iso ev wb-kelvin focus ae-lock ultrawide histogram \
        max-exposure-us stack-state stack-start trigger-autofocus; do
      ${lib.getBin gst_all_1.gstreamer}/bin/gst-inspect-1.0 fp6libcamerasrc | grep -q "$p" \
        || { echo "fp6libcamerasrc lacks $p" >&2; exit 1; }
    done
    timeout 60 $out/libexec/fp6camera-selftest
    test -s $TMPDIR/img.jpg
    runHook postInstallCheck
  '';

  meta = {
    description = "FP6 libcamera glue for Qt5 camerabin (lomiri-camera-app)";
    license = lib.licenses.lgpl21Plus;
    platforms = lib.platforms.linux;
  };
}
