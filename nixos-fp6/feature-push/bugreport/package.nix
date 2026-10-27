{ lib, rustPlatform, pkg-config, qt5, lomiri, systemd, iproute2, networkmanager
, wireplumber, coreutils, util-linux, glib, bash, withGui ? true }:
rustPlatform.buildRustPackage rec {
  pname = "fp6-bugreport";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./src ./tests ./qml ./nix ./LICENSE ];
  };
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ pkg-config ] ++ lib.optionals withGui [ qt5.wrapQtAppsHook ];
  buildInputs = lib.optionals withGui [ qt5.qtbase qt5.qtdeclarative qt5.qtgraphicaleffects lomiri.lomiri-ui-toolkit ];
  buildFeatures = lib.optional withGui "gui";
  dontWrapQtApps = true;
  FP6_SYSTEMD_BIN = "${systemd}/bin";
  FP6_IP_BIN = "${iproute2}/bin";
  FP6_NM_BIN = "${networkmanager}/bin";
  FP6_WP_BIN = "${wireplumber}/bin";
  FP6_COREUTILS_BIN = "${coreutils}/bin";
  FP6_UTIL_BIN = "${util-linux}/bin";
  FP6_GLIB_BIN = "${glib.bin}/bin";
  FP6_TEST_SH = "${bash}/bin/bash";
  preBuild = ''
    export FP6_HELPER_PATH="$out/libexec/fp6-bugreport-helper"
    export FP6_QML_PATH="$out/share/fp6-bugreport/qml/Main.qml"
  '';
  postInstall = ''
    mkdir -p "$out/libexec" "$out/share/fp6-bugreport"
    mv "$out/bin/fp6-bugreport-helper" "$out/libexec/"
    cp -r qml "$out/share/fp6-bugreport/"
    install -Dm644 nix/org.fairphone6.bugreport.policy "$out/share/polkit-1/actions/org.fairphone6.bugreport.policy"
    substituteInPlace "$out/share/polkit-1/actions/org.fairphone6.bugreport.policy" \
      --replace-fail '@HELPER@' "$out/libexec/fp6-bugreport-helper"
  '' + lib.optionalString withGui ''
    wrapQtApp "$out/bin/fp6-bugreport-gui" \
      --prefix XDG_DATA_DIRS : "${lomiri.suru-icon-theme}/share" \
      --prefix QT_PLUGIN_PATH : "${qt5.qtbase.bin}/${qt5.qtbase.qtPluginPrefix}" \
      --prefix QML2_IMPORT_PATH : "${lomiri.lomiri-ui-toolkit}/${qt5.qtbase.qtQmlPrefix}"
    install -Dm644 nix/fp6-bugreport.desktop "$out/share/applications/fp6-bugreport.desktop"
    substituteInPlace "$out/share/applications/fp6-bugreport.desktop" \
      --replace-fail '@SYSTEMCTL@' '${systemd}/bin/systemctl'
    install -Dm644 nix/bugreport.svg "$out/share/icons/hicolor/scalable/apps/fp6-bugreport.svg"
  '';
  meta = {
    description = "Private local Fairphone 6 / Lomiri diagnostics";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "fp6-bugreport";
  };
}
