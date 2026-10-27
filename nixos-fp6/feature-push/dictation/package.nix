{ lib, rustPlatform, cmake, pkg-config, clang, makeWrapper, dbus
, pipewire, curl, qt5, lomiri }:
rustPlatform.buildRustPackage {
  pname = "fp6-dictation";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml ./Cargo.lock ./models.json ./src ./tests ./qml
      ./scripts/settings-launcher.sh ./docs/com.fairphone6.Dictation1.xml
    ];
  };
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ cmake pkg-config clang rustPlatform.bindgenHook makeWrapper qt5.wrapQtAppsHook ];
  buildInputs = [ qt5.qtdeclarative qt5.qtquickcontrols2 qt5.qtgraphicaleffects lomiri.lomiri-ui-toolkit ];
  nativeCheckInputs = [ dbus ];
  # Portable binaries: never inherit build-host CPU tuning into a phone package.
  GGML_NATIVE = "OFF";
  dontWrapQtApps = true;
  postInstall = ''
    mkdir -p $out/share/fp6-dictation $out/share/applications $out/share/dbus-1/interfaces
    cp qml/Settings.qml $out/share/fp6-dictation/
    cp docs/com.fairphone6.Dictation1.xml $out/share/dbus-1/interfaces/
    substitute scripts/settings-launcher.sh $out/bin/fp6-dictation-settings \
      --replace-fail '@qml@' "$out/share/fp6-dictation" \
      --replace-fail 'exec qmlscene' 'exec ${qt5.qtdeclarative.dev}/bin/qmlscene'
    chmod +x $out/bin/fp6-dictation-settings
    wrapQtApp $out/bin/fp6-dictation-settings
    wrapProgram $out/bin/fp6-dictationd --prefix PATH : ${lib.makeBinPath [ pipewire ]}
    wrapProgram $out/bin/fp6-dictate --prefix PATH : ${lib.makeBinPath [ curl ]}
    cp qml/fp6-dictation.desktop $out/share/applications/
  '';
  meta = {
    description = "Offline PipeWire voice dictation for the Maliit keyboard";
    license = lib.licenses.mit;
    platforms = [ "x86_64-linux" "aarch64-linux" ];
    mainProgram = "fp6-dictate";
  };
}
