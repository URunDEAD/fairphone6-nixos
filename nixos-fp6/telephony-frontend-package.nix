{ lib, stdenv, fetchFromGitLab, cmake, pkg-config, gettext, wrapQtAppsHook
, qtbase, qtdeclarative, qtpim, qtsystems, qtmultimedia, qtgraphicaleffects
, qtquickcontrols2, libqofono, libnotify, lomiri-schemas, lomiri-ui-toolkit, lomiri-content-hub
, lomiri-indicator-network, lomiri-telephony-service, lomiri-history-service
, lomiri-thumbnailer, gsettings-qt, accounts-qml-module, buteo-syncfw
, addressbook, lomiri-online-accounts, qtcontacts-sqlite
, dbus, xvfb-run, mesa, makeFontsConf, dejavu_fonts, suru-icon-theme
, kind ? "dialer" }:
let
  messaging = kind == "messaging";
  repo = "lomiri-${kind}-app";
  pin = if messaging then {
    version = "2.1.1-unstable-2026-10-22";
    rev = "f685586b345e5dbce01dd2ae7447907570ea6c86";
    hash = "0987ljl2jck4jqqwb0lj2apjjdmpm7lnx7awlwy6drmmkq7p256s";
  } else {
    version = "2.0.0-unstable-2026-10-19";
    rev = "09fa56285a5cadab437c435ee6e4dde03a573308";
    hash = "0c8arc2v3mprmhhc6xny3fi3ggkw20ypsh13brshfjnb2912ncjb";
  };
in assert builtins.elem kind [ "dialer" "messaging" ]; stdenv.mkDerivation {
  pname = repo;
  inherit (pin) version;
  src = fetchFromGitLab {
    owner = "ubports";
    repo = "development/core/${repo}";
    inherit (pin) rev;
    sha256 = pin.hash;
  };
  nativeBuildInputs = [ cmake pkg-config gettext wrapQtAppsHook ];
  buildInputs = [ qtbase qtdeclarative qtpim qtsystems qtmultimedia qtgraphicaleffects
    qtquickcontrols2 libqofono libnotify lomiri-schemas lomiri-ui-toolkit lomiri-content-hub
    lomiri-indicator-network lomiri-telephony-service lomiri-history-service
    lomiri-thumbnailer gsettings-qt accounts-qml-module buteo-syncfw
    addressbook lomiri-online-accounts qtcontacts-sqlite suru-icon-theme ];
  nativeCheckInputs = [ dbus xvfb-run mesa.llvmpipeHook ];
  postPatch = ''
    # GNUInstallDirs may contain absolute paths (as in Nix). Prefixing BINDIR
    # again incorrectly makes an installed app choose the source-tree QML.
    substituteInPlace config.h.in \
      --replace-fail '@CMAKE_INSTALL_PREFIX@/@CMAKE_INSTALL_BINDIR@' '@CMAKE_INSTALL_FULL_BINDIR@' \
      --replace-fail '@CMAKE_INSTALL_PREFIX@/@${if messaging then "MESSAGING" else "DIALER"}_APP_DIR@/' \
        '@CMAKE_INSTALL_FULL_DATADIR@/${repo}/'
  '' + lib.optionalString messaging ''
    # qt5_use_modules requires these explicitly discovered targets on current
    # CMake/Qt; the old source relied on implicit discovery from dependencies.
    substituteInPlace CMakeLists.txt \
      --replace-fail 'find_package(Qt5DBus)' \
        'find_package(Qt5 REQUIRED COMPONENTS Core DBus Gui Multimedia Qml Quick Sql Versit Test QuickTest)'
    # The app imports no Unity8 private module. This obsolete configure probe
    # predates the Lomiri rename; normal QML imports resolve through wrappers.
    sed -i '/#find unity8 qml libraries/,/^endif()/c\set(UNITY8_QML_PATH "${lomiri-ui-toolkit}/${qtbase.qtQmlPrefix}")' CMakeLists.txt
    # KeyboardRectangle is implemented in this same directory, using Qt's
    # actual inputMethod geometry. No external Lomiri.Keyboard module exists.
    substituteInPlace src/qml/GroupChatInfoPage.qml \
      --replace-fail 'import Lomiri.Keyboard 0.1' ""
  '';
  preConfigure = ''
    qtWrapperArgs+=(--prefix XDG_DATA_DIRS : ${suru-icon-theme}/share)
    qtWrapperArgs+=(--prefix XDG_DATA_DIRS : ${lomiri-schemas}/share/gsettings-schemas/${lomiri-schemas.name})
  '';
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    export QT_QPA_PLATFORM=xcb
    export FONTCONFIG_FILE=${makeFontsConf { fontDirectories = [ dejavu_fonts ]; }}
    export UITK_ICON_THEME=suru
    export XDG_DATA_HOME="$TMPDIR/${kind}-data"
    export XDG_CONFIG_HOME="$TMPDIR/${kind}-config"
    export XDG_CACHE_HOME="$TMPDIR/${kind}-cache"
    export XDG_RUNTIME_DIR="$TMPDIR/${kind}-runtime"
    mkdir -m700 -p "$XDG_RUNTIME_DIR"
    export DBUS_SYSTEM_BUS_ADDRESS=unix:path="$XDG_RUNTIME_DIR/no-system-bus"
    for program in tests/qml/tst_QmlTests tests/private/DialPadSearchTest; do
      if [ -x "$program" ]; then wrapQtApp "$PWD/$program"; fi
    done
    timeout 240s dbus-run-session --config-file=${dbus}/share/dbus-1/session.conf -- \
      ctest -j1 --output-on-failure
    runHook postCheck
  '';
  enableParallelBuilding = true;
  meta = {
    description = "Native Lomiri ${kind} frontend with real Contacts/AddressBook middleware";
    homepage = "https://gitlab.com/ubports/development/core/${repo}";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = repo;
  };
}
