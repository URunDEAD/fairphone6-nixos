# FP6 sensor apps: Rust backend (std only, no crates) + a minimal Qt 5.15
# QML shim (app/, one QObject) + Lomiri UI Toolkit QML apps (qml/).
#
# camera: the FP6 lomiri-camera-app (camera/lomiri-camera-app-fp6.nix, with
# passthru qtmultimediaFp6/gstFp6Camera) and the phone's libcamera; when
# given, bin/fp6-magnifier gets the same camera stack environment as Lomiri
# Camera. Without them (laptop preview) the magnifier has no camera.
{ lib
, stdenv
, rustPlatform
, cmake
, libsForQt5
, lomiri
, librsvg
, pipewire
, glib
, gst_all_1
, camera ? null
, cameraLibcamera ? null
}:
let
  hasCamera = camera != null && camera ? qtmultimediaFp6 && cameraLibcamera != null;
  cameraWrapArgs = lib.optionals hasCamera [
    "--prefix" "NIXPKGS_QT5_QML_IMPORT_PATH" ":" "${camera.qtmultimediaFp6.bin}/${libsForQt5.qtbase.qtQmlPrefix}"
    "--prefix" "QT_PLUGIN_PATH" ":" "${camera.qtmultimediaFp6.bin}/${libsForQt5.qtbase.qtPluginPrefix}"
    "--prefix" "GST_PLUGIN_SYSTEM_PATH_1_0" ":" (lib.makeSearchPath "lib/gstreamer-1.0" (with gst_all_1; [
      gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad camera.gstFp6Camera cameraLibcamera ]))
    "--set" "QT_GSTREAMER_CAMERABIN_SRC" "fp6camerabinsrc"
    "--set" "QT_GSTREAMER_CAMERABIN_VIDEOSRC" "fp6libcamerasrc"
    "--set-default" "FP6_CAMERA_CONFIG" "/etc/fp6-camera/camera.conf"
    "--prefix" "LIBCAMERA_IPA_CONFIG_PATH" ":" "${camera.gstFp6Camera}/share/libcamera/ipa"
    "--set" "QT_GSTREAMER_USE_OPENGL_PLUGIN" "0"
    # same as Lomiri Camera: no Iris encoders (only the viewfinder is used)
    "--set" "GST_PLUGIN_FEATURE_RANK" "v4l2h264enc:NONE,v4l2h265enc:NONE,v4l2vp8enc:NONE,v4l2vp9enc:NONE"
  ];

  core = rustPlatform.buildRustPackage {
    pname = "fp6-sensors-core";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./rust;
      filter = path: type: !(lib.hasInfix "/target" path);
    };
    cargoLock.lockFile = ./rust/Cargo.lock;
    # cargo test runs the maths unit tests (WMM2025 test values, tilt
    # compensation, calibration, A-weighting, ISA altitude, UTM/MGRS, ...).
    doCheck = true;
    postInstall = ''
      lib=$(find target -path '*release/libfp6_sensors.a' | head -n1)
      test -n "$lib"
      install -Dm644 "$lib" $out/lib/libfp6_sensors.a
    '';
    meta.mainProgram = "fp6-sensor-cli";
  };
in
stdenv.mkDerivation {
  pname = "fp6-sensor-apps";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type:
      let rel = lib.removePrefix (toString ./. + "/") (toString path); in
      lib.any (p: rel == p || lib.hasPrefix (p + "/") rel) [ "app" "qml" "icons" ];
  };

  cmakeDir = "../app";

  nativeBuildInputs = [ cmake libsForQt5.wrapQtAppsHook librsvg ];
  buildInputs = with libsForQt5; [
    qtbase
    qtdeclarative
    qtgraphicaleffects
    qtsvg
    qtlocation          # QtPositioning (geoclue2 plugin)
    lomiri.lomiri-ui-toolkit
  ];

  cmakeFlags = [ "-DFP6_SENSORS_LIB=${core}/lib/libfp6_sensors.a" ];

  qtWrapperArgs = [
    "--set-default FP6_PW_RECORD ${lib.getExe' pipewire "pw-record"}"
    "--set-default FP6_PW_PLAY ${lib.getExe' pipewire "pw-play"}"
    "--set-default FP6_GDBUS ${lib.getExe' glib.bin "gdbus"}"
  ];

  # Wrapped by hand: fp6-magnifier also gets the camera stack environment.
  dontWrapQtApps = true;
  postFixup = ''
    wrapQtApp $out/bin/fp6-sensor-tools
    wrapQtApp $out/bin/fp6-sensor-cli
  '' + (if hasCamera then ''
    # The magnifier must see one QtMultimedia only, the camera's (a Qt input
    # propagates the stock one into qtWrapperArgs): swap it, like Lomiri
    # Camera's own wrapper check.
    saved=("''${qtWrapperArgs[@]}")
    qtWrapperArgs=()
    for a in "''${saved[@]}"; do
      qtWrapperArgs+=("''${a//${libsForQt5.qtmultimedia.bin}/${camera.qtmultimediaFp6.bin}}")
    done
    wrapQtApp $out/bin/fp6-magnifier ${lib.escapeShellArgs cameraWrapArgs}
    qtWrapperArgs=("''${saved[@]}")
    grep -q fp6camerabinsrc $out/bin/fp6-magnifier
    grep -q ${camera.qtmultimediaFp6.bin} $out/bin/fp6-magnifier
    ! grep -q ${libsForQt5.qtmultimedia.bin} $out/bin/fp6-magnifier
    ! grep -q fp6camerabinsrc $out/bin/fp6-sensor-tools
  '' else ''
    wrapQtApp $out/bin/fp6-magnifier
  '') + ''
    test -x $out/bin/.fp6-magnifier-wrapped
  '';

  postInstall = ''
    # Launcher icons: real PNG files rendered from the SVG sources.
    mkdir -p $out/share/fp6-sensor-apps/icons
    for s in ../icons/*.svg; do
      n=$(basename "$s" .svg)
      install -Dm644 "$s" $out/share/fp6-sensor-apps/icons/$n.svg
      rsvg-convert -w 256 -h 256 "$s" -o $out/share/fp6-sensor-apps/icons/$n.png
      # also as theme icons (Icon=fp6-compass, fp6-tools-folder, ...)
      install -Dm644 "$s" $out/share/icons/hicolor/scalable/apps/$n.svg
      install -Dm644 $out/share/fp6-sensor-apps/icons/$n.png $out/share/icons/hicolor/256x256/apps/$n.png
    done
    install -Dm755 ${core}/bin/fp6-sensor-cli $out/bin/fp6-sensor-cli
  '';

  passthru = {
    inherit core hasCamera;
    skyfeed = "${core}/bin/fp6-gnss-skyfeed";
  };

  meta = {
    description = "Sensor and utility apps (compass, level, ruler, altimeter, sound/light meter, GPS, dashboard, speedometer, metal detector, vibration meter, step counter, flashlight, magnifier, tuner, tone generator) for Lomiri on the Fairphone 6";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "fp6-sensor-tools";
  };
}
