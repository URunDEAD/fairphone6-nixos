# Standalone "Fingerprint ID" app: lomiri-settings-components' Fingerprint pages
# (add with guided progress, list, rename, delete, remove all) over biometryd.
# systemBusAddress != null pins the app's system bus (used only while biometryd
# runs on a private test bus); null = the real system bus.
{ lib, stdenv, qtbase, qtdeclarative, wrapQtAppsHook, glib
, lomiri-ui-toolkit, lomiri-settings-components, gsettings-qt, biometryd, lomiri-schemas
, systemBusAddress ? null }:
stdenv.mkDerivation {
  pname = "fp6-fingerprint-id";
  version = "0.1.0";
  src = ./.;
  nativeBuildInputs = [ wrapQtAppsHook ];
  buildInputs = [ qtbase qtdeclarative lomiri-ui-toolkit lomiri-settings-components gsettings-qt biometryd ];
  dontConfigure = true;
  buildPhase = ''
    runHook preBuild
    $CXX -std=c++17 -O2 -fPIC -DFP6_QML_DIR="\"$out/share/fp6-fingerprint-id\"" main.cpp -o fp6-fingerprint-id \
      -I${qtbase.dev}/include -I${qtbase.dev}/include/QtCore -I${qtbase.dev}/include/QtGui \
      -I${qtdeclarative.dev}/include -I${qtdeclarative.dev}/include/QtQuick -I${qtdeclarative.dev}/include/QtQml \
      -L${qtbase.out}/lib -L${qtdeclarative.out}/lib -lQt5Quick -lQt5Qml -lQt5Gui -lQt5Core
    runHook postBuild
  '';
  installPhase = ''
    runHook preInstall
    install -Dm755 fp6-fingerprint-id $out/bin/fp6-fingerprint-id
    install -Dm644 Main.qml $out/share/fp6-fingerprint-id/Main.qml
    install -Dm644 /dev/stdin $out/share/applications/fp6-fingerprint-id.desktop <<DESKTOP
    [Desktop Entry]
    Type=Application
    Name=Fingerprint ID
    Comment=Add, list and remove fingerprints
    Icon=lomiri-system-settings
    Exec=$out/bin/fp6-fingerprint-id
    Terminal=false
    X-Lomiri-Touch=true
    X-Lomiri-Single-Instance=true
    DESKTOP
    sed -i 's/^    //' $out/share/applications/fp6-fingerprint-id.desktop
    runHook postInstall
  '';
  qtWrapperArgs = [
    "--prefix" "XDG_DATA_DIRS" ":" "${glib.getSchemaDataDirPath lomiri-schemas}"
  ] ++ lib.optionals (systemBusAddress != null) [
    "--set" "DBUS_SYSTEM_BUS_ADDRESS" systemBusAddress
  ];
  meta = {
    description = "Fingerprint ID (Lomiri Settings fingerprint pages) as a standalone app";
    license = lib.licenses.lgpl3Only;
    platforms = lib.platforms.linux;
    mainProgram = "fp6-fingerprint-id";
  };
}
