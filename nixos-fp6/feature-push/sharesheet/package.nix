{ lib, stdenv, rustPlatform, pkg-config, qt5, lomiri, dbus, makeWrapper, cups ? null }:
rustPlatform.buildRustPackage {
  pname = "fp6-share";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ./qml ./data ./tests ./LICENSE ];
  };
  cargoLock.lockFile = ./Cargo.lock;
  buildFeatures = [ "ui" ];
  nativeBuildInputs = [ pkg-config qt5.qmake qt5.wrapQtAppsHook makeWrapper ];
  buildInputs = [ qt5.qtbase qt5.qtdeclarative qt5.qtgraphicaleffects qt5.qtsvg lomiri.lomiri-ui-toolkit lomiri.lomiri-content-hub ];
  nativeCheckInputs = [ dbus ];
  # Bus tests explicitly opt in to private synthetic services.
  preCheck = ''export FP6_SHARE_BUS_TESTS=1'';
  checkPhase = ''
    runHook preCheck
    dbus-run-session --config-file tests/session-bus.conf -- cargo test --locked --release --features ui --target ${stdenv.hostPlatform.rust.rustcTarget} -j "$NIX_BUILD_CORES"
    runHook postCheck
  '';
  postInstall = ''
    mkdir -p $out/share/fp6-share/qml $out/share/applications $out/share/lomiri-content-hub/peers $out/share/dbus-1/services
    cp qml/*.qml $out/share/fp6-share/qml/
    cp data/com.fairphone6.Share.desktop $out/share/applications/
    substituteInPlace $out/share/applications/com.fairphone6.Share.desktop \
      --replace-fail 'Exec=fp6-share-ui' "Exec=$out/bin/fp6-share-ui"
    cp data/com.fairphone6.Share $out/share/lomiri-content-hub/peers/
    cp data/com.fairphone6.Share1.service $out/share/dbus-1/services/
    substituteInPlace $out/share/dbus-1/services/com.fairphone6.Share1.service \
      --replace-fail 'Exec=fp6-share daemon' "Exec=$out/bin/fp6-share daemon"
    wrapProgram $out/bin/fp6-share --prefix PATH : ${lib.makeBinPath (lib.optional (cups != null) cups)}
    qtWrapperArgs+=(
      --set-default APP_ID "com.fairphone6.Share"
      --set FP6_SHARE_QML_DIR "$out/share/fp6-share/qml"
      --prefix XDG_DATA_DIRS : "${lomiri.suru-icon-theme}/share"
      --prefix QML2_IMPORT_PATH : "${lomiri.lomiri-ui-toolkit}/${qt5.qtbase.qtQmlPrefix}:${lomiri.lomiri-content-hub}/${qt5.qtbase.qtQmlPrefix}"
    )
  '';
  meta = {
    description = "Unified share sheet for Lomiri";
    license = lib.licenses.gpl3Plus;
    platforms = [ "x86_64-linux" "aarch64-linux" ];
    mainProgram = "fp6-share";
  };
}
