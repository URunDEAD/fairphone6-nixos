{ lib, stdenv, rustPlatform, qt5, makeWrapper, dbus, lomiriUiToolkit ? null,
  suruIconTheme ? null, withGui ? true }:
rustPlatform.buildRustPackage {
  pname = "fp6-llm";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ./data ./qml ./settings-page ./tests/runtime.rs ./tests/support ./nix ./scripts ./LICENSE ./README.md ];
  };
  cargoLock.lockFile = ./Cargo.lock;
  buildFeatures = lib.optional withGui "ui";
  nativeBuildInputs = [ makeWrapper ] ++ lib.optionals withGui [ qt5.qmake qt5.wrapQtAppsHook ];
  buildInputs = lib.optionals withGui [ qt5.qtbase qt5.qtdeclarative qt5.qtgraphicaleffects qt5.qtquickcontrols2 qt5.qtsvg ]
    ++ lib.optional (withGui && lomiriUiToolkit != null) lomiriUiToolkit;
  nativeCheckInputs = [ dbus ];
  checkPhase = ''
    runHook preCheck
    dbus-run-session --config-file=${dbus}/share/dbus-1/session.conf -- cargo test --offline --release --target ${stdenv.hostPlatform.rust.rustcTarget} ${lib.optionalString withGui "--features ui"}
    runHook postCheck
  '';
  dontWrapQtApps = true;
  postInstall = ''
    rm -f "$out/bin/fake-llama-server"
    mkdir -p "$out/share/fp6-llm" "$out/share/doc/fp6-llm"
    cp data/models.json "$out/share/fp6-llm/"
    cp README.md LICENSE "$out/share/doc/fp6-llm/"
    install -Dm644 nix/com.fairphone6.LocalLlm1.service "$out/share/dbus-1/services/com.fairphone6.LocalLlm1.service"
    substituteInPlace "$out/share/dbus-1/services/com.fairphone6.LocalLlm1.service" --replace-fail '@out@' "$out"
    install -Dm755 scripts/benchmark-phone "$out/bin/fp6-llm-benchmark-phone"
    substituteInPlace "$out/bin/fp6-llm-benchmark-phone" --replace-fail '@out@' "$out"
  '' + lib.optionalString withGui ''
    module="$out/${qt5.qtbase.qtQmlPrefix}/Fairphone/LocalLlm"
    mkdir -p "$module" "$out/share/fp6-llm/qml"
    install -m644 nix/qmldir qml/ModelsView.qml "$module/"
    install -m755 "$out/lib/libfp6_llm.so" "$module/libfp6_llm.so"
    rm -f "$out/lib/libfp6_llm.so"
    cp qml/Main.qml "$out/share/fp6-llm/qml/"
    install -Dm644 settings-page/local-llm/local-llm.settings "$out/share/lomiri-system-settings/local-llm.settings"
    install -Dm644 settings-page/local-llm/PageComponent.qml "$out/share/lomiri-system-settings/qml-plugins/local-llm/PageComponent.qml"
    install -Dm644 nix/com.fairphone6.local-llm.desktop "$out/share/applications/com.fairphone6.local-llm.desktop"
    substituteInPlace "$out/share/applications/com.fairphone6.local-llm.desktop" --replace-fail 'Exec=fp6-llm-settings' "Exec=$out/bin/fp6-llm-settings"
    wrapProgram "$out/bin/fp6-llm-settings" "''${qtWrapperArgs[@]}" \
      --prefix QML2_IMPORT_PATH : "$out/${qt5.qtbase.qtQmlPrefix}" \
      --set FP6_LLM_QML "$out/share/fp6-llm/qml/Main.qml" \
      --set QML_DISABLE_DISK_CACHE 1 --set UITK_ICON_THEME suru \
      ${lib.optionalString (suruIconTheme != null) "--prefix XDG_DATA_DIRS : ${suruIconTheme}/share"}
  '';
  meta = { description = "Local model downloads, lifecycle and OpenAI chat endpoint for Fairphone 6"; license = lib.licenses.mit; platforms = lib.platforms.linux; mainProgram = "fp6-llm"; };
}
