# FP6.ScreenRecorder QML module (C++: a Qt Quick item has to live in Lomiri's
# process and render thread), loaded by the patched Shell.qml.
{ lib, stdenv, cmake, libsForQt5, recorder, recordWidth ? 720, recordFps ? 30 }:
stdenv.mkDerivation {
  pname = "fp6-screenrecorder-qml";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./CMakeLists.txt ./qmldir ./plugin.cpp ./screenrecorder.cpp ./screenrecorder.h ];
  };
  nativeBuildInputs = [ cmake ];
  buildInputs = with libsForQt5; [ qtbase qtdeclarative ];
  dontWrapQtApps = true;
  cmakeFlags = [
    "-DFP6_SCREENRECORD_BIN=${lib.getExe' recorder "fp6-screenrecord"}"
    "-DFP6_RECORD_WIDTH=${toString recordWidth}"
    "-DFP6_RECORD_FPS=${toString recordFps}"
    "-DQML_INSTALL_DIR=${libsForQt5.qtbase.qtQmlPrefix}"
  ];
  meta = {
    description = "Lomiri screen recorder QML item (frame read-back, hands frames to fp6-screenrecord)";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
