# FP6.Accessibility QML module: ColourMatrix (inversion / colour correction
# matrices for FP6.DisplayComfort ColorFilter.preMatrix), Magnifier (screen
# magnification gestures) and the MagnifierControl singleton. Loaded by the
# patched ShellScreen.qml / quick-settings backend inside Lomiri's process.
# doCheck: the colour maths unit test and the gesture test (offscreen QPA)
# when building natively.
{ lib, stdenv, cmake, libsForQt5, runTests ? stdenv.buildPlatform.canExecute stdenv.hostPlatform }:
stdenv.mkDerivation {
  pname = "fp6-accessibility-qml";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./CMakeLists.txt ./qmldir ./plugin.cpp
      ./colourmath.cpp ./colourmath.h ./colourmatrix.cpp ./colourmatrix.h ./magnifier.cpp ./magnifier.h
      ./tests/colourmath_test.cpp ./tests/magnifier_test.cpp ];
  };
  nativeBuildInputs = [ cmake ];
  buildInputs = with libsForQt5; [ qtbase qtdeclarative ];
  dontWrapQtApps = true;
  cmakeFlags = [
    "-DQML_INSTALL_DIR=${libsForQt5.qtbase.qtQmlPrefix}"
    "-DFP6_BUILD_TESTS=${if runTests then "ON" else "OFF"}"
  ];
  doCheck = runTests;
  checkPhase = ''
    runHook preCheck
    ./colourmath-test
    export QT_PLUGIN_PATH=${lib.getBin libsForQt5.qtbase}/${libsForQt5.qtbase.qtPluginPrefix}
    export QML2_IMPORT_PATH=${libsForQt5.qtdeclarative}/${libsForQt5.qtbase.qtQmlPrefix}
    HOME=$TMPDIR QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software ./magnifier-test
    runHook postCheck
  '';
  meta = {
    description = "Lomiri screen magnification and colour correction QML items (FP6)";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
