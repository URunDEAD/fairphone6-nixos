{ lib, rustPlatform, stdenvNoCC, makeWrapper, qt5, lomiri, tesseract }:
let
  version = "0.1.0";
  engine = tesseract.override { enableLanguages = [ "eng" "ron" ]; };
  pluginPath = lib.makeSearchPath "lib/qt-${qt5.qtbase.version}/plugins" [ qt5.qtbase.bin qt5.qtwayland ];
  qmlPath = lib.makeSearchPath "lib/qt-${qt5.qtbase.version}/qml" [
    qt5.qtdeclarative qt5.qtgraphicaleffects qt5.qtquickcontrols
    lomiri.lomiri-ui-toolkit lomiri.lomiri-content-hub
  ];
  # Keep compilation separate from UI/desktop installation so packaging-only
  # changes do not require rebuilding all Rust dependencies.
  rustBin = rustPlatform.buildRustPackage {
    pname = "fairphone-ocr";
    inherit version;
    src = lib.cleanSourceWith {
      src = ./.;
      filter = path: _type:
        let
          relative = lib.removePrefix "${toString ./.}/" (toString path);
          top = builtins.head (lib.splitString "/" relative);
        in toString path == toString ./. || builtins.elem top [
          "Cargo.toml" "Cargo.lock" "LICENSE" "src" "tests" "examples"
        ];
    };
    cargoHash = "sha256-qa66wLcLlaKV2NqodP8+1K7Inzv/NvckZ07N0EJNg1U=";
    doCheck = true;
  };
in stdenvNoCC.mkDerivation {
  pname = "fairphone-ocr";
  inherit version;
  dontUnpack = true;
  nativeBuildInputs = [ makeWrapper ];
  installPhase = ''
    runHook preInstall
    mkdir -p $out/bin $out/share/fairphone-ocr $out/share/applications \
      $out/share/dbus-1/services $out/share/lomiri-content-hub/peers \
      $out/share/icons/hicolor/scalable/apps
    ln -s ${rustBin}/bin/fairphone-ocr $out/bin/fairphone-ocr
    cp ${./qml}/*.qml $out/share/fairphone-ocr/
    cp ${./data}/fairphone-ocr.desktop $out/share/applications/
    substituteInPlace $out/share/applications/fairphone-ocr.desktop \
      --replace-fail 'Exec=fairphone-ocr' "Exec=$out/bin/fairphone-ocr"
    # Native Content Hub peer filenames are app IDs, without a .json suffix.
    cp ${./data}/fairphone-ocr.json $out/share/lomiri-content-hub/peers/fairphone-ocr
    cp ${./data}/fairphone-ocr.svg $out/share/icons/hicolor/scalable/apps/
    substitute ${./data}/com.fairphone6.Ocr1.service $out/share/dbus-1/services/com.fairphone6.Ocr1.service \
      --subst-var-by out $out
    wrapProgram $out/bin/fairphone-ocr \
      --set OCR_TESSERACT ${engine}/bin/tesseract \
      --set OCR_QML_DIR $out/share/fairphone-ocr \
      --set OCR_QMLSCENE ${qt5.qtdeclarative.dev}/bin/qmlscene \
      --prefix QML2_IMPORT_PATH : ${qmlPath} \
      --prefix QT_PLUGIN_PATH : ${pluginPath}
    runHook postInstall
  '';
  passthru = { inherit rustBin; };
  meta = {
    description = "Private offline English and Romanian OCR for Lomiri";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "fairphone-ocr";
  };
}
