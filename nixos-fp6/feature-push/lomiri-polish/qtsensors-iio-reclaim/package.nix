# Patched QtSensors iio-sensor-proxy orientation backend, built out of tree as
# a separate plugin with its own identifier so nothing else is rebuilt:
#   fp6-iio-sensor-proxy.orientationsensor
# Differences from upstream (iio-sensor-proxy-reclaim.patch):
#  * registered even when net.hadess.SensorProxy is not running yet;
#  * stays active and claims again when the proxy restarts (NameOwnerChanged)
#    or when HasAccelerometer turns true;
#  * handles a direct owner hand-over (serviceOwnerChanged).
{ lib, stdenv, qmake, qtbase, qtsensors, pkg-config, dbus, python3 }:
let
  python = python3.withPackages (p: [ p.dbus-python p.pygobject3 ]);
in
stdenv.mkDerivation {
  pname = "qtsensors-fp6-iio-sensor-proxy";
  version = qtsensors.version;
  inherit (qtsensors) src;

  nativeBuildInputs = [ qmake pkg-config ];
  buildInputs = [ qtbase qtsensors ];
  dontWrapQtApps = true;

  # Only the iio-sensor-proxy backend directory is built.
  postUnpack = ''
    sourceRoot="$sourceRoot/src/plugins/sensors/iio-sensor-proxy"
  '';
  patches = [ ./iio-sensor-proxy-reclaim.patch ];
  postPatch = ''
    cp ${./fp6-iio-sensor-proxy.pro} fp6-iio-sensor-proxy.pro
    rm iio-sensor-proxy.pro
  '';
  qmakeFlags = [ "PREFIX=${placeholder "out"}/${qtbase.qtPluginPrefix}" "fp6-iio-sensor-proxy.pro" ];

  doInstallCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  installCheckPhase = ''
    runHook preInstallCheck
    $CXX -std=c++14 -fPIC ${./reclaim-test.cpp} -o reclaim-test \
      $(pkg-config --cflags --libs Qt5Sensors Qt5DBus Qt5Core)
    export QT_PLUGIN_PATH=$out/${qtbase.qtPluginPrefix}
    export HOME=$TMPDIR XDG_CONFIG_DIRS=$TMPDIR/none XDG_CONFIG_HOME=$TMPDIR/none
    ${dbus}/bin/dbus-run-session \
      --dbus-daemon=${dbus}/bin/dbus-daemon \
      --config-file=${dbus}/share/dbus-1/session.conf -- \
      sh -c 'DBUS_SYSTEM_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS exec ./reclaim-test ${python}/bin/python3 ${./fake-sensorproxy.py}'
    runHook postInstallCheck
  '';

  meta = {
    description = "QtSensors iio-sensor-proxy orientation backend that survives proxy restarts (FP6)";
    license = with lib.licenses; [ lgpl3Only gpl2Plus ];
    platforms = lib.platforms.linux;
  };
}
