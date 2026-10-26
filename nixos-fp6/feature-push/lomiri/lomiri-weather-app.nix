# Lomiri Weather App 6.3.0 (UBports core app), not packaged in pinned nixpkgs.
# Upstream links Qt5WebEngine only for OnlineMap.qml, whose single entry point
# (HeaderRow.qml) is commented out upstream. Qt5 qtwebengine is marked insecure
# in nixpkgs and would have to be rebuilt from source against the FP6 patched
# qtdeclarative, so the unused WebEngine view is dropped here. The weather data
# provider is Open-Meteo (no API key, no account).
{ stdenv, lib, fetchurl, cmake, gettext, pkg-config, wrapQtAppsHook
, flatbuffers, qtbase, qtdeclarative, qtquickcontrols2, qtlocation, qtpositioning
, qtgraphicaleffects, pyotherside, python3
, lomiri-ui-toolkit, lomiri-indicator-network, suru-icon-theme }:
stdenv.mkDerivation (finalAttrs: {
  pname = "lomiri-weather-app";
  version = "6.3.0";

  src = fetchurl {
    url = "https://gitlab.com/ubports/development/apps/lomiri-weather-app/-/archive/v${finalAttrs.version}/lomiri-weather-app-v${finalAttrs.version}.tar.gz";
    sha256 = "d71b36aae5ac15dcc8777b52390c6a9d86ac4b1c59946e4de8f606be4018021a";
  };

  postPatch = ''
    # qmake -query QT_INSTALL_QML points into qtdeclarative's own prefix.
    substituteInPlace CMakeLists.txt \
      --replace-fail 'COMMAND ''${QMAKE_EXECUTABLE} -query QT_INSTALL_QML' \
                     'COMMAND echo ''${CMAKE_INSTALL_PREFIX}/${qtbase.qtQmlPrefix}' \
      --replace-fail 'find_package(Qt5WebEngine REQUIRED)' ""
    substituteInPlace src/app/CMakeLists.txt \
      --replace-fail ' Qt5::WebEngine)' ')'
    substituteInPlace src/app/main.cpp \
      --replace-fail '#include <QtWebEngine>' '#include <QQmlEngine>
#include <QFile>' \
      --replace-fail 'QtWebEngine::initialize();' ""
    substituteInPlace src/app/qml/qml.qrc \
      --replace-fail '<file>components/OnlineMap.qml</file>' ""
    # The file stays on disk only for the translation template (po/ lists it);
    # it is no longer compiled into the app's resources.
    # The only reference stays commented out; fail loudly if upstream changes that.
    if grep -rn 'OnlineMap' src/app/qml --include=*.qml | grep -v '//.*OnlineMap'; then
      echo 'OnlineMap is referenced by live QML; WebEngine removal is no longer safe' >&2
      exit 1
    fi
    # No absolute paths in the desktop entry (lomiri-app-launch looks them up).
    substituteInPlace lomiri-weather-app.desktop.in.in \
      --replace-fail '@ICON@' 'lomiri-weather-app' \
      --replace-fail '@SPLASH@' 'lomiri-app-launch/splash/lomiri-weather-app.svg'
  '';

  strictDeps = true;
  nativeBuildInputs = [ cmake gettext pkg-config wrapQtAppsHook flatbuffers ];
  buildInputs = [
    flatbuffers qtbase qtdeclarative qtquickcontrols2 qtlocation qtpositioning
    # QML runtime imports
    qtgraphicaleffects pyotherside lomiri-ui-toolkit lomiri-indicator-network suru-icon-theme
  ];

  cmakeFlags = [
    (lib.cmakeBool "CLICK_MODE" false)
    (lib.cmakeBool "CONNECTIVITY_CHECK" true)
  ];

  # AboutTroubleshooting.qml runs python3 through PyOtherSide.
  qtWrapperArgs = [ "--prefix" "PATH" ":" (lib.makeBinPath [ python3 ]) ];

  postInstall = ''
    mkdir -p $out/share/icons/hicolor/scalable/apps $out/share/lomiri-app-launch/splash
    ln -s $out/share/lomiri-weather-app/weather-app.svg \
      $out/share/icons/hicolor/scalable/apps/lomiri-weather-app.svg
    ln -s $out/share/lomiri-weather-app/weather-app-splash.svg \
      $out/share/lomiri-app-launch/splash/lomiri-weather-app.svg
    test -x $out/bin/lomiri-weather-app
    test -e $out/${qtbase.qtQmlPrefix}/LomiriWeather/qmldir
  '';

  meta = {
    description = "Weather app for Lomiri (UBports), Open-Meteo provider, WebEngine map removed";
    homepage = "https://gitlab.com/ubports/development/apps/lomiri-weather-app";
    license = lib.licenses.gpl3Only;
    mainProgram = "lomiri-weather-app";
    platforms = lib.platforms.linux;
  };
})
