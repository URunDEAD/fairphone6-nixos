# GStreamer glue between Qt5 camerabin (lomiri-camera-app) and libcamera.
# Elements: fp6camerabinsrc (camera selection/orientation/flash) and
# fp6libcamerasrc (fixed-size libcamerasrc + scaler). Offline self test runs
# on the native builder with videotestsrc and a temp file as the "LED".
{ lib, stdenv, pkg-config, gst_all_1, libraw, exiftool, python3 }:
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
      -Werror=incompatible-pointer-types $flags -o libgstfp6camera.so fp6camera.c fp6-dng.c $libs
    $CC -O2 -Wall $flags $(pkg-config --cflags gstreamer-pbutils-1.0) -o fp6camera-selftest \
      fp6camera-selftest.c $libs $(pkg-config --libs gstreamer-pbutils-1.0)
    # manual controls / long exposure from the command line (live camera)
    $CC -O2 -Wall -DFP6_TUNING_DIR=\"$out/share/libcamera/ipa\" $(pkg-config --cflags gstreamer-1.0) -o fp6-long-exposure \
      fp6camera-manualtest.c $(pkg-config --libs gstreamer-1.0)
    # phase 2 (RAW/DNG, capture jobs) from the command line (live camera)
    $CC -O2 -Wall -DFP6_TUNING_DIR=\"$out/share/libcamera/ipa\" $(pkg-config --cflags gstreamer-1.0) -o fp6-capture \
      fp6camera-capturetest.c $(pkg-config --libs gstreamer-1.0)
    # offline DNG writer check (install check)
    $CC -O2 -Wall $(pkg-config --cflags glib-2.0) -o fp6-dng-selftest fp6-dng-selftest.c fp6-dng.c \
      $(pkg-config --libs glib-2.0) -lm
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    install -Dm755 libgstfp6camera.so $out/lib/gstreamer-1.0/libgstfp6camera.so
    install -Dm755 fp6camera-selftest $out/libexec/fp6camera-selftest
    install -Dm755 fp6-long-exposure $out/bin/fp6-long-exposure
    install -Dm755 fp6-capture $out/bin/fp6-capture
    install -Dm755 fp6-dng-selftest $out/libexec/fp6-dng-selftest
    # softISP tuning for the sensors the libcamera package has none for;
    # used through LIBCAMERA_IPA_CONFIG_PATH=$out/share/libcamera/ipa
    install -Dm644 -t $out/share/libcamera/ipa/simple tuning/ov13b10.yaml tuning/fp6-s5kkd1.yaml
    runHook postInstall
  '';

  doInstallCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  nativeInstallCheckInputs = [ libraw exiftool python3 ];
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
    # phase 2: properties, DNG writer (decoded by dcraw, tags by exiftool)
    for p in raw-active raw-format raw-state raw-capture sensor; do
      ${lib.getBin gst_all_1.gstreamer}/bin/gst-inspect-1.0 fp6libcamerasrc | grep -q "$p" \
        || { echo "fp6libcamerasrc lacks $p" >&2; exit 1; }
    done
    $out/libexec/fp6-dng-selftest $TMPDIR/t.dng
    raw-identify -v $TMPDIR/t.dng | tee $TMPDIR/dcraw.txt
    grep -qi "filter pattern: *BG/*GR" $TMPDIR/dcraw.txt
    grep -qi "image size: *64 x 48" $TMPDIR/dcraw.txt
    exiftool -s -Orientation# -CFAPattern2 -BlackLevel -WhiteLevel -ISO -ExposureTime \
      -ColorMatrix1 -UniqueCameraModel $TMPDIR/t.dng | tee $TMPDIR/exif.txt
    grep -q "Orientation *: 6" $TMPDIR/exif.txt
    grep -q "CFAPattern2 *: 2 1 1 0" $TMPDIR/exif.txt
    grep -q "ColorMatrix1 *: 1 -0.25 0 0 1 0 0 0 1" $TMPDIR/exif.txt
    grep -q "ISO *: 250" $TMPDIR/exif.txt
    # colours: half-size raw-colour render, rotated by dcraw (orientation 6)
    dcraw_emu -h -o 0 -r 1 1 1 1 -W -t 0 $TMPDIR/t.dng
    python3 - $TMPDIR/t.dng.ppm <<'PY'
    import sys
    d = open(sys.argv[1], 'rb').read()
    parts = d.split(maxsplit=4)
    w, h = int(parts[1]), int(parts[2]); px = parts[4][-w * h * 3:]
    def at(x, y): i = 3 * (y * w + x); return tuple(px[i:i + 3])
    q = {k: at(int(fx * w), int(fy * h)) for k, (fx, fy) in
         {'tl': (.25, .25), 'tr': (.75, .25), 'bl': (.25, .75), 'br': (.75, .75)}.items()}
    print(w, h, q)
    # -t 0: unrotated; TL red, TR green, BL blue, BR grey
    assert q['tl'][0] > 3 * max(q['tl'][1:]), q
    assert q['tr'][1] > 3 * max(q['tr'][0], q['tr'][2]), q
    assert q['bl'][2] > 3 * max(q['bl'][:2]), q
    assert min(q['br']) > 0.7 * max(q['br']), q
    PY
    test -s $TMPDIR/img.jpg
    runHook postInstallCheck
  '';

  meta = {
    description = "FP6 libcamera glue for Qt5 camerabin (lomiri-camera-app)";
    license = lib.licenses.lgpl21Plus;
    platforms = lib.platforms.linux;
  };
}
