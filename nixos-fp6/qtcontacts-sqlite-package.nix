{ lib, stdenv, fetchFromGitLab, qmake, pkg-config, qtbase, qtpim, dbus, sqlite, sqlite-icu-extension }:
stdenv.mkDerivation {
  pname = "qtcontacts-sqlite";
  version = "0.3.5";
  src = fetchFromGitLab {
    owner = "ubports";
    repo = "development/core/qtcontacts-sqlite";
    rev = "ea00c0b6f986d122d919dacad573f4213dd877ea";
    sha256 = "1wk5rd4ckww7p6vmkwlxwkcgj6c5n5xjnq1044pvsn48g6a0v5wa";
  };
  patches = [ ./qtcontacts-sqlite-qt515-test.patch ];
  nativeBuildInputs = [ qmake pkg-config ];
  buildInputs = [ qtbase qtpim sqlite ];
  nativeCheckInputs = [ dbus ];
  dontWrapQtApps = true;
  postPatch = ''
    substituteInPlace src/engine/engine.pro \
      --replace-fail 'target.path = $$[QT_INSTALL_PLUGINS]/contacts' \
        'target.path = ${placeholder "out"}/${qtbase.qtPluginPrefix}/contacts'
    substituteInPlace src/engine/contactsdatabase.cpp \
      --replace-fail '"libSqliteIcu"' '"${sqlite-icu-extension}/lib/libSqliteIcu.so"'
    substituteInPlace config.pri --replace-fail '-flto' '-flto=2' \
      --replace-fail '$$[QT_INSTALL_LIBS]/qtcontacts-sqlite-qt5/' '$''${PREFIX}/lib/qtcontacts-sqlite-qt5/'
    # No EDS migration or benchmarks are needed for the local native backend.
    substituteInPlace qtcontacts-sqlite.pro --replace-fail 'SUBDIRS += migration' ""
    substituteInPlace tests/tests.pro --replace-fail 'auto \' 'auto' \
      --replace-fail '    benchmarks' ""
    substituteInPlace tests/run_test.sh \
      --replace-fail 'dbus-daemon --session' \
        'dbus-daemon --config-file=${dbus}/share/dbus-1/session.conf' \
      --replace-fail 'src/engine/' 'src/engine/:${lib.getBin qtbase}/${qtbase.qtPluginPrefix}'
  '';
  qmakeFlags = [ "CONFIG+=load_icu" "PREFIX=${placeholder "out"}" ];
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    export QT_QPA_PLATFORM=offscreen
    export DBUS_SYSTEM_BUS_ADDRESS=unix:path="$TMPDIR/no-system-bus"
    timeout 240s make -C tests/auto -j1 check
    runHook postCheck
  '';
  installPhase = ''
    runHook preInstall
    make -C src install
    runHook postInstall
  '';
  enableParallelBuilding = true;
  meta = {
    description = "Official UBports local persistent QtContacts SQLite backend";
    homepage = "https://gitlab.com/ubports/development/core/qtcontacts-sqlite";
    license = lib.licenses.lgpl21Plus;
    platforms = lib.platforms.linux;
  };
}
