{ lib, rustPlatform, qt5, makeWrapper, nftables, iputils, curl, coreutils,
  lomiriUiToolkit ? null, suruIconTheme ? null }:
rustPlatform.buildRustPackage {
  pname = "fp6-netpolicy";
  version = "0.1.0";
  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type:
      let relative = lib.removePrefix (toString ./. + "/") (toString path);
          top = builtins.head (lib.splitString "/" relative);
      in (path == ./. || builtins.elem top [ "Cargo.toml" "Cargo.lock" "src" "qml" "tests" "examples" "nix" "LICENSE" "README.md" "DESIGN.md" ])
        && lib.cleanSourceFilter path type;
  };
  cargoLock.lockFile = ./Cargo.lock;
  buildFeatures = [ "ui" ];
  nativeBuildInputs = [ qt5.qmake qt5.wrapQtAppsHook makeWrapper ];
  buildInputs = [ qt5.qtbase qt5.qtdeclarative qt5.qtquickcontrols2 qt5.qtgraphicaleffects qt5.qtsvg ]
    ++ lib.optional (lomiriUiToolkit != null) lomiriUiToolkit;
  dontWrapQtApps = true;
  postInstall = ''
    mkdir -p $out/share/fp6-netpolicy $out/share/applications
    cp -r qml $out/share/fp6-netpolicy/
    cp nix/org.fairphone.netpolicy.desktop $out/share/applications/
    substituteInPlace $out/share/applications/org.fairphone.netpolicy.desktop \
      --replace-fail 'Exec=netpolicy-ui' "Exec=$out/bin/netpolicy-ui"
    wrapProgram $out/bin/netpolicy-ui "''${qtWrapperArgs[@]}" \
      --set NETPOLICY_QML "$out/share/fp6-netpolicy/qml/Main.qml" \
      --set UITK_ICON_THEME suru ${lib.optionalString (suruIconTheme != null) "--prefix XDG_DATA_DIRS : ${suruIconTheme}/share"}
    mkdir -p $out/share/doc/fp6-netpolicy
    cp README.md DESIGN.md LICENSE $out/share/doc/fp6-netpolicy/
    wrapProgram $out/bin/netpolicy-helper \
      --prefix PATH : ${lib.makeBinPath [ nftables iputils curl coreutils ]} \
      --set CURL_CA_BUNDLE ${curl.cacert or "/etc/ssl/certs/ca-certificates.crt"}
  '';
  meta = {
    description = "Data saver and smart Wi-Fi/mobile switching for Lomiri";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "netpolicy-ui";
  };
}
