{ lib, stdenv, fetchFromGitLab, cmake, pkg-config, gettext, wrapQtAppsHook
, qtbase, qtdeclarative, qtpim, qtsystems, libqofono
, lomiri-schemas, lomiri-ui-toolkit, lomiri-content-hub, lomiri-indicator-network
, lomiri-telephony-service, lomiri-online-accounts, accounts-qml-module
, buteo-syncfw, qtcontacts-sqlite, dbus, xvfb-run, makeFontsConf, dejavu_fonts, suru-icon-theme, mesa }:
stdenv.mkDerivation {
  pname = "lomiri-addressbook-app";
  version = "0.9.1-unstable-2026-10-23";
  src = fetchFromGitLab {
    owner = "ubports";
    repo = "development/core/lomiri-addressbook-app";
    rev = "298e67d0f15ac537e3619a50fc9b4772d99ed857";
    sha256 = "0qh61nzl1g4386s6nwah7nqz00i1yg6rhhv7j01vcw1q60gahxy0";
  };
  nativeBuildInputs = [ cmake pkg-config gettext wrapQtAppsHook ];
  buildInputs = [ qtbase qtdeclarative qtpim qtsystems libqofono
    lomiri-schemas lomiri-ui-toolkit lomiri-content-hub lomiri-indicator-network
    lomiri-telephony-service lomiri-online-accounts accounts-qml-module buteo-syncfw qtcontacts-sqlite suru-icon-theme ];
  nativeCheckInputs = [ dbus xvfb-run mesa.llvmpipeHook ];
  postPatch = ''
    substituteInPlace CMakeLists.txt \
      --replace-fail 'set(QT_IMPORTS_DIR "''${CMAKE_INSTALL_LIBDIR}/qt5/qml")' \
                     'set(QT_IMPORTS_DIR "${qtbase.qtQmlPrefix}")'
    # Upstream restricts discovery to Debian's /usr/lib multiarch layout.
    substituteInPlace tests/qml/CMakeLists.txt \
      --replace-fail 'PATHS /usr/lib/*/qt5/bin' 'PATHS ${qtdeclarative.dev}/bin' \
      --replace-fail 'NO_DEFAULT_PATH' "" \
      --replace-fail 'if(QMLTESTRUNNER_BIN AND XVFB_RUN_BIN)' 'if(QMLTESTRUNNER_BIN)'
  '';
  cmakeFlags = [ "-DUSE_XVFB=OFF" "-DENABLE_EDS_IMPORTER=OFF" ];
  preConfigure = ''
    qtWrapperArgs+=(--prefix XDG_DATA_DIRS : ${suru-icon-theme}/share)
    qtWrapperArgs+=(--prefix XDG_DATA_DIRS : ${lomiri-schemas}/share/gsettings-schemas/${lomiri-schemas.name})
    makeQtWrapper ${qtdeclarative.dev}/bin/qmltestrunner "$TMPDIR/addressbook-qmltestrunner"
    cmakeFlagsArray+=("-DQMLTESTRUNNER_BIN=$TMPDIR/addressbook-qmltestrunner")
  '';
  doCheck = true;
  # Full upstream QML suite runs against real native plugins on an isolated
  # bus and disposable contact storage; no phone/user data or daemon activation.
  checkPhase = ''
    runHook preCheck
    export QT_QPA_PLATFORM=xcb
    export FONTCONFIG_FILE=${makeFontsConf { fontDirectories = [ dejavu_fonts ]; }}
    export UITK_ICON_THEME=suru
    export XDG_DATA_HOME="$TMPDIR/addressbook-data"
    export XDG_CONFIG_HOME="$TMPDIR/addressbook-config"
    export XDG_CACHE_HOME="$TMPDIR/addressbook-cache"
    export XDG_RUNTIME_DIR="$TMPDIR/addressbook-runtime"
    mkdir -m700 -p "$XDG_RUNTIME_DIR"
    export DBUS_SYSTEM_BUS_ADDRESS=unix:path="$XDG_RUNTIME_DIR/no-system-bus"
    export QML2_IMPORT_PATH="$PWD/src/imports:$QML2_IMPORT_PATH"
    timeout 180s xvfb-run -a -s "-screen 0 1024x768x24" dbus-run-session --config-file=${dbus}/share/dbus-1/session.conf -- ctest -j1 --output-on-failure
    runHook postCheck
  '';
  enableParallelBuilding = true;
  meta = {
    description = "Lomiri address book application and real AddressBook/Contacts QML bindings";
    homepage = "https://gitlab.com/ubports/development/core/lomiri-addressbook-app";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "lomiri-addressbook-app";
  };
}
