# Lomiri System Settings part of the FP6 wallpapers: the Fp6.Wallpapers QML
# module (C++, lock-screen copy in the greeter data dir) and the extra QML
# files of the Background page. Added through the wrapper's `plugins`;
# MainPage.qml/Preview.qml are patched in the wrapper (lockscreen-polish-fp6.nix).
{ lib, stdenv, cmake, libsForQt5, fp6-lockscreen-schema, greeterDataDir ? "/var/lib/lightdm-data" }:
stdenv.mkDerivation {
  pname = "lomiri-system-settings-fp6-wallpapers";
  version = "1";
  src = ./.;
  strictDeps = true;
  nativeBuildInputs = [ cmake ];
  buildInputs = [ libsForQt5.qtbase libsForQt5.qtdeclarative ];
  dontWrapQtApps = true;
  cmakeDir = "../plugin";
  cmakeFlags = [ "-DFP6_GREETER_DATA_DIR=${greeterDataDir}" ];
  postInstall = ''
    q=$out/share/lomiri-system-settings/qml-plugins/background
    install -Dm644 -t $q ${./qml}/*.qml
    mkdir -p $out/share/locale $out/share/gsettings-schemas
    ln -s ${fp6-lockscreen-schema}/share/gsettings-schemas/${fp6-lockscreen-schema.name} $out/share/gsettings-schemas/
    test -e $out/lib/lomiri-system-settings/private/Fp6/Wallpapers/libfp6wallpapers.so
    test -e $out/lib/lomiri-system-settings/private/Fp6/Wallpapers/qmldir
  '';
  meta = {
    description = "Separate lock-screen and home-screen wallpapers for Lomiri System Settings";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
  };
}
