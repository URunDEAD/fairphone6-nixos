# FP6 sensor apps: Rust backend (std only, no crates) + a minimal Qt 5.15
# QML shim (app/, one QObject) + Lomiri UI Toolkit QML apps (qml/).
{ lib
, stdenv
, rustPlatform
, cmake
, libsForQt5
, lomiri
, librsvg
, pipewire
, glib
}:
let
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
    "--set-default FP6_GDBUS ${lib.getExe' glib.bin "gdbus"}"
  ];

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
    inherit core;
    skyfeed = "${core}/bin/fp6-gnss-skyfeed";
  };

  meta = {
    description = "Compass, level, ruler, altimeter, sound/light meter, GPS status and sensor dashboard for Lomiri on the Fairphone 6";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "fp6-sensor-tools";
  };
}
