{ lib, rustPlatform, protobuf, cmake, pkg-config, qt5, lomiri, dbus }:
rustPlatform.buildRustPackage {
  pname = "fp6-quickshare";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type:
      let name = baseNameOf path; in
      !(builtins.elem name [ ".git" ".commit-history" "target" "result" "build" "app-build" ])
      && !(lib.hasPrefix "result-" name)
      && !(lib.hasSuffix ".png" name)
      && !(lib.hasSuffix ".md" name);
  };
  cargoLock.lockFile = ./Cargo.lock;
  nativeBuildInputs = [ protobuf cmake pkg-config qt5.wrapQtAppsHook ];
  buildInputs = [
    qt5.qtbase qt5.qtdeclarative qt5.qtquickcontrols qt5.qtgraphicaleffects qt5.qtsvg qt5.qtsvg.bin
    lomiri.lomiri-ui-toolkit lomiri.lomiri-content-hub lomiri.suru-icon-theme
  ];
  nativeCheckInputs = [ dbus ];
  dontUseCmakeConfigure = true;
  postBuild = ''
    cmake -S app -B app-build -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=$out
    cmake --build app-build --parallel $NIX_BUILD_CORES
  '';
  postInstall = ''
    cmake --install app-build
    mkdir -p $out/share/fp6-quickshare $out/share/applications \
      $out/share/content-hub/peers $out/share/icons/hicolor/scalable/apps \
      $out/share/dbus-1/services $out/share/systemd/user
    cp -r app/qml $out/share/fp6-quickshare/
    cp data/com.fairphone6.QuickShare.desktop $out/share/applications/
    cp data/com.fairphone6.QuickShare.json $out/share/content-hub/peers/
    cp data/fp6-quickshare.svg $out/share/icons/hicolor/scalable/apps/
    cp data/com.fairphone6.QuickShare1.service $out/share/dbus-1/services/
    cp data/fp6-quickshare.service $out/share/systemd/user/
    substituteInPlace $out/share/applications/com.fairphone6.QuickShare.desktop \
      --replace-fail 'Exec=fp6-quickshare-app' "Exec=$out/bin/fp6-quickshare-app"
    substituteInPlace $out/share/dbus-1/services/com.fairphone6.QuickShare1.service \
      --replace-fail 'Exec=fp6-quickshare-daemon' "Exec=$out/bin/fp6-quickshare-daemon"
    substituteInPlace $out/share/systemd/user/fp6-quickshare.service \
      --replace-fail 'ExecStart=fp6-quickshare-daemon' "ExecStart=$out/bin/fp6-quickshare-daemon"
  '';
  # Only the GUI needs Qt; the daemon starts it for explicit notification actions.
  preFixup = ''
    qtWrapperArgs+=(--prefix PATH : "$out/bin" --prefix XDG_DATA_DIRS : "${lomiri.suru-icon-theme}/share")
  '';
  meta = {
    description = "Quick Share LAN daemon, CLI and Lomiri application for Fairphone 6";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
    mainProgram = "fp6-quickshare";
  };
}
