{ lib, rustPlatform, pkg-config, openssl, qt5, lomiri, dbus, makeWrapper, pipewire }:
rustPlatform.buildRustPackage {
  pname = "lomiri-kdeconnect";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ./qml ./data ./tests ./LICENSE ];
  };
  cargoLock.lockFile = ./Cargo.lock;
  buildFeatures = [ "ui" ];
  nativeBuildInputs = [ pkg-config qt5.qmake qt5.wrapQtAppsHook makeWrapper ];
  buildInputs = [ openssl qt5.qtbase qt5.qtdeclarative qt5.qtgraphicaleffects qt5.qtsvg
    lomiri.lomiri-ui-toolkit lomiri.lomiri-content-hub ];
  nativeCheckInputs = [ dbus ];
  doCheck = true;
  postInstall = ''
    mkdir -p $out/share/lomiri-kdeconnect/qml $out/share/applications $out/share/lomiri-content-hub/peers
    cp qml/*.qml $out/share/lomiri-kdeconnect/qml/
    cp data/com.fairphone6.KdeConnect.desktop $out/share/applications/
    substituteInPlace $out/share/applications/com.fairphone6.KdeConnect.desktop \
      --replace-fail 'Exec=lomiri-kdeconnect' "Exec=$out/bin/lomiri-kdeconnect"
    cp data/com.fairphone6.KdeConnect.json $out/share/lomiri-content-hub/peers/com.fairphone6.KdeConnect
    wrapProgram $out/bin/lomiri-kdeconnectd --prefix PATH : ${lib.makeBinPath [ pipewire ]}
    qtWrapperArgs+=(
      --set-default APP_ID "com.fairphone6.KdeConnect"
      --set KDECONNECT_QML_DIR "$out/share/lomiri-kdeconnect/qml"
      --prefix XDG_DATA_DIRS : "${lomiri.suru-icon-theme}/share"
      --prefix QML2_IMPORT_PATH : "${lomiri.lomiri-ui-toolkit}/${qt5.qtbase.qtQmlPrefix}:${lomiri.lomiri-content-hub}/${qt5.qtbase.qtQmlPrefix}"
    )
  '';
  meta = {
    description = "KDE Connect companion for the Lomiri mobile shell";
    license = lib.licenses.gpl3Plus;
    platforms = [ "x86_64-linux" "aarch64-linux" ];
    mainProgram = "lomiri-kdeconnect";
  };
}
