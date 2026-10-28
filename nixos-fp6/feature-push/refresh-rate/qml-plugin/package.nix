# FP6.RefreshRate QML module (PanelRate, ActivityWatcher), loaded by
# Lomiri's Fp6RefreshRate.qml. doCheck runs the offscreen test when built
# natively (the laptop check builds it for x86_64: ../test/default.nix).
{ lib, stdenv, cmake, libsForQt5, runTests ? stdenv.buildPlatform.canExecute stdenv.hostPlatform }:
stdenv.mkDerivation {
  pname = "fp6-refreshrate-qml";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./CMakeLists.txt ./qmldir ./plugin.cpp
      ./panelrate.cpp ./panelrate.h ./activitywatcher.cpp ./activitywatcher.h
      ./tests/refreshrate_test.cpp ];
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
    export QT_PLUGIN_PATH=${lib.getBin libsForQt5.qtbase}/${libsForQt5.qtbase.qtPluginPrefix}
    HOME=$TMPDIR QT_QPA_PLATFORM=offscreen ./refreshrate-test
    runHook postCheck
  '';
  meta = {
    description = "Lomiri refresh-rate helpers: panel runtime rate (sysfs) and input activity";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
