{ lib, rustPlatform, cmake, pkg-config, clang, makeWrapper, dbus
, pipewire, curl, qt5, lomiri, espeak-ng, ffmpeg }:
rustPlatform.buildRustPackage {
  pname = "fp6-captions";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml ./Cargo.lock ./models.json ./src ./tests ./qml ./docs/com.fairphone6.Captions1.xml
    ];
  };
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ cmake pkg-config clang rustPlatform.bindgenHook makeWrapper qt5.wrapQtAppsHook ];
  buildInputs = [ qt5.qtbase qt5.qtdeclarative qt5.qtquickcontrols2 qt5.qtgraphicaleffects lomiri.lomiri-ui-toolkit ];
  nativeCheckInputs = [ dbus espeak-ng ffmpeg ];
  GGML_NATIVE = "OFF";
  dontWrapQtApps = true;
  postInstall = ''
    mkdir -p $out/share/applications $out/share/dbus-1/interfaces
    cp qml/fp6-captions.desktop $out/share/applications/
    cp docs/com.fairphone6.Captions1.xml $out/share/dbus-1/interfaces/
    wrapQtApp $out/bin/fp6-captions-ui
    wrapProgram $out/bin/fp6-captions --prefix PATH : ${lib.makeBinPath [ pipewire curl ]}
  '';
  meta = {
    description = "Private offline live captions for Lomiri";
    license = lib.licenses.mit;
    platforms = [ "x86_64-linux" "aarch64-linux" ];
    mainProgram = "fp6-captions-ui";
  };
}
