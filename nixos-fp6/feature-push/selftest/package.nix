{ lib, rustPlatform, qt5, makeWrapper, dbus, glib, bash, coreutils, systemd,
  procps, wireplumber, lomiriUiToolkit ? null, suruIconTheme ? null, withGui ? true }:
rustPlatform.buildRustPackage {
  pname = "fp6-selftest";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ./data ./qml ./settings-page ./tests ./nix ./LICENSE ./README.md ./DESIGN.md ];
  };
  cargoLock.lockFile = ./Cargo.lock;
  buildFeatures = lib.optional withGui "ui";
  nativeBuildInputs = [ makeWrapper ] ++ lib.optionals withGui [ qt5.qmake qt5.wrapQtAppsHook ];
  buildInputs = lib.optionals withGui [ qt5.qtbase qt5.qtdeclarative qt5.qtgraphicaleffects qt5.qtquickcontrols2 qt5.qtsvg ]
    ++ lib.optional (withGui && lomiriUiToolkit != null) lomiriUiToolkit;
  nativeCheckInputs = [ dbus glib bash coreutils ];
  dontWrapQtApps = true;
  postInstall = ''
    mkdir -p "$out/share/fp6-selftest" "$out/share/doc/fp6-selftest"
    cp data/probes.toml "$out/share/fp6-selftest/"
    cp README.md DESIGN.md LICENSE "$out/share/doc/fp6-selftest/"
    wrapProgram "$out/bin/fp6-selftest" \
      --prefix PATH : ${lib.makeBinPath [ systemd procps wireplumber coreutils glib ]}
  '' + lib.optionalString withGui ''
    cp -r qml "$out/share/fp6-selftest/"
    install -Dm644 nix/com.fairphone6.selftest.desktop "$out/share/applications/com.fairphone6.selftest.desktop"
    substituteInPlace "$out/share/applications/com.fairphone6.selftest.desktop" --replace-fail 'Exec=fp6-selftest-ui' "Exec=$out/bin/fp6-selftest-ui"
    install -Dm644 nix/fp6-selftest.svg "$out/share/icons/hicolor/scalable/apps/fp6-selftest.svg"
    install -Dm644 settings-page/selftest/selftest.settings "$out/share/lomiri-system-settings/selftest.settings"
    install -Dm644 settings-page/selftest/PageComponent.qml "$out/share/lomiri-system-settings/qml-plugins/selftest/PageComponent.qml"
    wrapProgram "$out/bin/fp6-selftest-ui" "''${qtWrapperArgs[@]}" \
      --set FP6_SELFTEST_QML "$out/share/fp6-selftest/qml/Main.qml" \
      --set QML_DISABLE_DISK_CACHE 1 \
      --set UITK_ICON_THEME suru ${lib.optionalString (suruIconTheme != null) "--prefix XDG_DATA_DIRS : ${suruIconTheme}/share"}
  '';
  meta = {
    description = "Private post-update health checks for Fairphone 6 and Lomiri";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "fp6-selftest";
  };
}
