{ lib, stdenv, fetchFromGitLab, cmake, pkg-config, qtbase, telepathy
, ofono-qt, sqlite, libphonenumber, protobuf, dbus, python3, glib }:
stdenv.mkDerivation {
  pname = "telepathy-ofono";
  version = "0.4.0";
  src = fetchFromGitLab {
    owner = "ubports";
    repo = "development/core/telepathy-ofono";
    rev = "c099aa8df27213b4ee8e942251f77305d31e01ea";
    sha256 = "05yv6lciqy4cw2nl3w8726jpv9q2mj63awy6lnn0491h2v5xb7fy";
  };
  nativeBuildInputs = [ cmake pkg-config ];
  buildInputs = [ qtbase telepathy ofono-qt sqlite libphonenumber protobuf ];
  nativeCheckInputs = [ dbus (python3.withPackages (p: [ p.pygobject3 ])) glib ];
  dontWrapQtApps = true;
  enableParallelBuilding = true;
  postPatch = ''
    # The separately shipped Android RIL account plugins assume getprop and
    # /ril_N modem paths. They are not a native QRTR account provider.
    substituteInPlace CMakeLists.txt \
      --replace-fail 'COMPONENTS Core DBus Network)' 'COMPONENTS Core DBus Network Sql)' \
      --replace-fail 'pkg_check_modules(MC_PLUGINS REQUIRED mission-control-plugins)' "" \
      --replace-fail 'add_subdirectory(mc-plugin)' ""
  '';
  doCheck = stdenv.buildPlatform.canExecute stdenv.hostPlatform;
  checkPhase = ''
    runHook preCheck
    export QT_QPA_PLATFORM=offscreen
    ctest --output-on-failure -R '^PhoneUtilsTest$'
    export TP_OFONO_SQLITE_DBPATH=:memory:
    export XDG_DATA_HOME="$TMPDIR/telepathy-data"
    export XDG_CACHE_HOME="$TMPDIR/telepathy-cache"
    for scenario in voice-only sim-registered; do
      timeout 25s dbus-run-session --config-file=${dbus}/share/dbus-1/session.conf -- \
        python3 ${./telephony-privatebus-check.py} \
        "$PWD/telepathy-ofono" "$scenario"
    done
    runHook postCheck
  '';
  meta = {
    description = "UBports oFono Telepathy connection manager, with explicit native account setup";
    homepage = "https://gitlab.com/ubports/development/core/telepathy-ofono";
    license = lib.licenses.lgpl3;
    platforms = lib.platforms.linux;
  };
}
