# FP6.DisplayComfort QML module: the ColorFilter item (night light +
# greyscale), loaded by the patched ShellScreen.qml inside Lomiri's process.
# doCheck runs the pixel test (xvfb + Mesa llvmpipe) when building natively.
{ lib, stdenv, cmake, libsForQt5, xvfb-run, mesa, runTests ? stdenv.buildPlatform.canExecute stdenv.hostPlatform }:
stdenv.mkDerivation {
  pname = "fp6-displaycomfort-qml";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./CMakeLists.txt ./qmldir ./plugin.cpp ./colorfilter.cpp ./colorfilter.h ./tests/colorfilter_test.cpp ];
  };
  nativeBuildInputs = [ cmake ] ++ lib.optional runTests xvfb-run;
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
    export QML2_IMPORT_PATH=${libsForQt5.qtdeclarative}/${libsForQt5.qtbase.qtQmlPrefix}
    export LD_LIBRARY_PATH=${mesa}/lib __GLX_VENDOR_LIBRARY_NAME=mesa
    export __EGL_VENDOR_LIBRARY_DIRS=${mesa}/share/glvnd/egl_vendor.d
    export LIBGL_ALWAYS_SOFTWARE=1 HOME=$TMPDIR QT_QPA_PLATFORM=xcb
    # Desktop GL (blend + copy paths) and GLES through EGL.
    xvfb-run -a -s '-screen 0 640x480x24' ./colorfilter-test --expect-path copy
    QT_XCB_GL_INTEGRATION=xcb_egl QT_OPENGL=es2 xvfb-run -a -s '-screen 0 640x480x24' ./colorfilter-test
    runHook postCheck
  '';
  meta = {
    description = "Lomiri full-screen colour filter (night light, greyscale) QML item";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
