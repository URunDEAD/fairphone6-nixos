{ lib, rustPlatform, qt5, lomiri, systemd, makeWrapper }:
rustPlatform.buildRustPackage {
  pname = "fp6-webshare";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type:
      lib.cleanSourceFilter path type
      && !(builtins.elem (builtins.baseNameOf path) [ "target" ".cargo" "patches" "previews" "result" "FINAL_REPORT.md" ".agents" ".codex" ".aws" ]);
  };
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ qt5.wrapQtAppsHook makeWrapper ];
  buildInputs = [ qt5.qtbase qt5.qtdeclarative qt5.qtgraphicaleffects qt5.qtquickcontrols2 lomiri.lomiri-ui-toolkit lomiri.lomiri-content-hub ];
  dontWrapQtApps = true;
  postInstall = ''
    mkdir -p $out/share/fp6-webshare $out/share/applications \
      $out/share/lomiri-content-hub/peers $out/share/icons/hicolor/scalable/apps \
      $out/share/dbus-1/interfaces $out/share/doc/fp6-webshare
    cp -r qml $out/share/fp6-webshare/
    substitute data/fp6-webshare.desktop $out/share/applications/fp6-webshare.desktop --subst-var out
    cp data/fp6-webshare.content-hub $out/share/lomiri-content-hub/peers/fp6-webshare
    cp data/fp6-webshare.svg $out/share/icons/hicolor/scalable/apps/
    cp data/com.fairphone6.WebShare1.xml $out/share/dbus-1/interfaces/
    cp README.md DESIGN.md $out/share/doc/fp6-webshare/
  '';
  postFixup = ''
    wrapQtApp $out/bin/fp6-webshare \
      --prefix XDG_DATA_DIRS : ${lomiri.suru-icon-theme}/share \
      --set FP6_WEBSHARE_QML_DIR $out/share/fp6-webshare/qml \
      --prefix PATH : ${lib.makeBinPath [ qt5.qtdeclarative.dev systemd ]}
  '';
  meta = {
    description = "Temporary, consent-based Wi-Fi sharing for Lomiri";
    license = lib.licenses.mit;
    platforms = [ "aarch64-linux" "x86_64-linux" ];
    mainProgram = "fp6-webshare";
  };
}
