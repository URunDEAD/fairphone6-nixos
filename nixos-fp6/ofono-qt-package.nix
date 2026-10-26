{ lib, stdenv, fetchFromGitHub, qmake, pkg-config, qtbase }:
stdenv.mkDerivation {
  pname = "ofono-qt";
  version = "1.30";
  src = fetchFromGitHub {
    owner = "Kaffeine";
    repo = "libofono-qt";
    rev = "8199e2832fe17c9838caf933ae2a4ae333eca517";
    sha256 = "1y9h6xprmq9bc84wrhlcdjkg88h7akwbyihr8g3ysn7hz444kry8";
  };
  nativeBuildInputs = [ qmake pkg-config ];
  propagatedBuildInputs = [ qtbase ];
  dontWrapQtApps = true;
  # Upstream's original tests control a running phonesim over the system
  # bus. The dependent telepathy-ofono package instead checks the real
  # library against synthetic oFono on a private disposable bus.
  qmakeFlags = [ "lib/lib.pro" ];
  postPatch = ''
    # This one public header still uses Qt keyword aliases, while its UBports
    # consumer compiles with QT_NO_KEYWORDS. Q_SIGNALS/Q_SLOTS keep the ABI.
    substituteInPlace lib/ofonocellbroadcast.h \
      --replace-fail 'signals:' 'Q_SIGNALS:' \
      --replace-fail 'public slots:' 'public Q_SLOTS:' \
      --replace-fail 'private slots:' 'private Q_SLOTS:'
    substituteInPlace lib/lib.pro \
      --replace-fail '$$[QT_INSTALL_PREFIX]/lib' "$out/lib" \
      --replace-fail '$$[QT_INSTALL_PREFIX]/include/ofono-qt' "$out/include/ofono-qt" \
      --replace-fail '$$[QT_INSTALL_ARCHDATA]/mkspecs/features' "$out/mkspecs/features"
    cat >> lib/lib.pro <<EOF
    QMAKE_PKGCONFIG_PREFIX = $out
    QMAKE_PKGCONFIG_LIBDIR = $out/lib
    EOF
  '';
  enableParallelBuilding = true;
  meta = {
    description = "Qt5 oFono bindings used by UBports telepathy-ofono (distinct from libqofono)";
    homepage = "https://github.com/Kaffeine/libofono-qt";
    license = lib.licenses.lgpl21;
    platforms = lib.platforms.linux;
  };
}
