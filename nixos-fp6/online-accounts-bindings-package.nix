{ lib, stdenv, fetchFromGitLab, cmake, pkg-config, qmake
, qtbase, qtdeclarative, accounts-qt, signond, libapparmor, dbus }:
stdenv.mkDerivation {
  pname = "lomiri-online-accounts-bindings";
  version = "0.21-unstable-2026-10-23";
  src = fetchFromGitLab {
    owner = "ubports";
    repo = "development/core/lomiri-online-accounts";
    rev = "1cbbac059b52a847805acdd76d242e7fe44cbd9e";
    sha256 = "1bsdhc8papxl0w49whf74prly5fczap6bamahkjhg63kd5kg3pc5";
  };
  nativeBuildInputs = [ cmake pkg-config qmake ];
  buildInputs = [ qtbase qtdeclarative accounts-qt signond libapparmor ];
  nativeCheckInputs = [ dbus ];
  dontWrapQtApps = true;
  postPatch = ''
    # Build the real API libraries and QML client modules only. This opt-in
    # prerequisite installs no daemon, D-Bus activation service, click hooks,
    # account UI, authentication plugin or fabricated account provider.
    substituteInPlace CMakeLists.txt \
      --replace-fail 'add_subdirectory(click-hooks)' "" \
      --replace-fail 'add_subdirectory(online-accounts-plugins)' "" \
      --replace-fail 'add_subdirectory(online-accounts-service)' "" \
      --replace-fail 'add_subdirectory(online-accounts-ui)' "" \
      --replace-fail 'add_subdirectory(po)' ""
    # The qmake query identifies Qt's immutable source prefix, not our prefix.
    substituteInPlace CMakeLists.txt \
      --replace-fail 'pkg_check_modules(APPARMOR libapparmor REQUIRED)' \
        'set(QT_INSTALL_QML "''${CMAKE_INSTALL_PREFIX}/${qtbase.qtQmlPrefix}")
pkg_check_modules(APPARMOR libapparmor REQUIRED)'
    cat >> CMakeLists.txt <<'CMAKE'
find_package(Qt5 COMPONENTS Test REQUIRED)
enable_testing()
add_subdirectory(tests/online-accounts-client/lib/tst_client)
CMAKE
  '';
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    export QT_QPA_PLATFORM=minimal
    export XDG_DATA_HOME="$TMPDIR/accounts-test-data"
    export XDG_CONFIG_HOME="$TMPDIR/accounts-test-config"
    export XDG_CACHE_HOME="$TMPDIR/accounts-test-cache"
    export DBUS_SYSTEM_BUS_ADDRESS=unix:path="$TMPDIR/no-system-bus"
    export QT_PLUGIN_PATH=${lib.getBin qtbase}/${qtbase.qtPluginPrefix}
    timeout 40s dbus-run-session --config-file=${dbus}/share/dbus-1/session.conf -- \
      tests/online-accounts-client/lib/tst_client/tst_client
    runHook postCheck
  '';
  cmakeFlags = [ "-DENABLE_DOC=OFF" "-DBUILD_TESTING=OFF" ];
  enableParallelBuilding = true;
  meta = {
    description = "Real Lomiri OnlineAccounts and Client Qt5 library/QML bindings, without services";
    homepage = "https://gitlab.com/ubports/development/core/lomiri-online-accounts";
    license = lib.licenses.lgpl3Plus;
    platforms = lib.platforms.linux;
  };
}
