# Wi-Fi QR codes: share a saved network / scan a code to join.
#  core:      Rust crate (rust/): payload format, QR matrix, QR decoding
#             (staticlib for the Qt parts) + the fp6-wifi-qr test CLI.
#  lssPlugin: System Settings pages (settings-page/*.qml, merged into the
#             stock Wi-Fi plugin directory) and the Fp6.WifiQr QML module
#             (QrCode item, WifiShare: lock check over PAM, NetworkManager
#             GetSecrets, scanner launcher). No QtMultimedia.
#  scanner:   fp6-wifi-qr-scan (Qt5/UITK app): camera viewfinder, frame
#             filter -> Rust decoder, NetworkManager join.
#
# camera: the FP6 lomiri-camera-app (camera/lomiri-camera-app-fp6.nix, with
# passthru qtmultimediaFp6/gstFp6Camera) and the phone's libcamera; then the
# scanner is built against that QtMultimedia and wrapped with Lomiri
# Camera's camera stack environment (as the Magnifier). Without them (laptop
# preview) it uses the stock QtMultimedia and has no phone camera.
{ lib
, stdenv
, rustPlatform
, cmake
, libsForQt5
, linux-pam
, lomiri
, librsvg
, gst_all_1
, camera ? null
, cameraLibcamera ? null
# NetworkManager 802-11-wireless.assigned-mac-address for new networks
# ("stable" with privacy-lock's Wi-Fi MAC policy); "" = NM's global default.
, assignedMac ? ""
}:
let
  hasCamera = camera != null && camera ? qtmultimediaFp6 && cameraLibcamera != null;
  qtmultimedia = if hasCamera then camera.qtmultimediaFp6 else libsForQt5.qtmultimedia;
  cameraWrapArgs = lib.optionals hasCamera [
    "--prefix" "QT_PLUGIN_PATH" ":" "${camera.qtmultimediaFp6.bin}/${libsForQt5.qtbase.qtPluginPrefix}"
    "--prefix" "GST_PLUGIN_SYSTEM_PATH_1_0" ":" (lib.makeSearchPath "lib/gstreamer-1.0" (with gst_all_1; [
      gstreamer.out gst-plugins-base.out gst-plugins-good.out gst-plugins-bad.out camera.gstFp6Camera cameraLibcamera ]))
    "--set" "QT_GSTREAMER_CAMERABIN_SRC" "fp6camerabinsrc"
    "--set" "QT_GSTREAMER_CAMERABIN_VIDEOSRC" "fp6libcamerasrc"
    "--set-default" "FP6_CAMERA_CONFIG" "/etc/fp6-camera/camera.conf"
    "--prefix" "LIBCAMERA_IPA_CONFIG_PATH" ":" "${camera.gstFp6Camera}/share/libcamera/ipa"
    "--set" "QT_GSTREAMER_USE_OPENGL_PLUGIN" "0"
    # viewfinder only: no Iris encoders (as Lomiri Camera and the Magnifier)
    "--set" "GST_PLUGIN_FEATURE_RANK" "v4l2h264enc:NONE,v4l2h265enc:NONE,v4l2vp8enc:NONE,v4l2vp9enc:NONE"
  ];

  core = rustPlatform.buildRustPackage {
    pname = "fp6-wifi-qr-core";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./rust;
      filter = path: _type: baseNameOf path != "target";
    };
    cargoLock.lockFile = ./rust/Cargo.lock;
    # cargo test (synthetic SSIDs/passwords only): escaping, encode/parse
    # round trips, other writers' variants, enterprise/invalid codes, QR
    # encode -> render -> decode (also rotated in a noisy camera-like frame),
    # the C ABI.
    doCheck = true;
    postInstall = ''
      lib=$(find target -path '*release/libfp6_wifi_qr.a' | head -n1)
      test -n "$lib"
      install -Dm644 "$lib" $out/lib/libfp6_wifi_qr.a
    '';
    meta = {
      description = "Wi-Fi QR payloads, QR encoding and decoding (FP6 Lomiri port)";
      license = lib.licenses.gpl3Only;
      mainProgram = "fp6-wifi-qr";
      platforms = lib.platforms.linux;
    };
  };

  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: _type:
      let rel = lib.removePrefix (toString ./. + "/") (toString path); in
      lib.any (p: rel == p || lib.hasPrefix (p + "/") rel) [ "qt" "scanner-qml" "settings-page" "icons" ];
  };

  lssPlugin = stdenv.mkDerivation {
    pname = "lomiri-system-settings-fp6-wifi-qr";
    version = "0.1.0";
    inherit src;
    cmakeDir = "../qt";
    nativeBuildInputs = [ cmake ];
    buildInputs = [ libsForQt5.qtbase libsForQt5.qtdeclarative linux-pam ];
    dontWrapQtApps = true;
    cmakeFlags = [ "-DFP6_BUILD_SCANNER=OFF" "-DFP6_WIFIQR_LIB=${core}/lib/libfp6_wifi_qr.a" ];
    postInstall = ''
      wifi=$out/share/lomiri-system-settings/qml-plugins/wifi
      install -Dm644 -t $wifi ../settings-page/*.qml
      install -Dm644 -t $wifi/fp6-icons ../icons/fp6-qrcode-symbolic.svg ../icons/fp6-fingerprint-symbolic.svg
      # The wrapper lndir's these from every plugin.
      mkdir -p $out/share/locale $out/lib/lomiri-system-settings
      test -e $out/lib/lomiri-system-settings/private/Fp6/WifiQr/libfp6wifiqrplugin.so
      test -e $out/lib/lomiri-system-settings/private/Fp6/WifiQr/qmldir
    '';
    passthru = { inherit core; };
    meta = {
      description = "Share Wi-Fi networks as QR codes from Lomiri System Settings";
      license = lib.licenses.gpl3Only;
      platforms = lib.platforms.linux;
    };
  };

  scanner = stdenv.mkDerivation {
    pname = "fp6-wifi-qr-scan";
    version = "0.1.0";
    inherit src;
    cmakeDir = "../qt";
    nativeBuildInputs = [ cmake libsForQt5.wrapQtAppsHook librsvg ];
    buildInputs = [
      libsForQt5.qtbase libsForQt5.qtdeclarative libsForQt5.qtgraphicaleffects libsForQt5.qtsvg
      qtmultimedia lomiri.lomiri-ui-toolkit
    ];
    cmakeFlags = [
      "-DFP6_BUILD_PLUGIN=OFF"
      "-DFP6_WIFIQR_LIB=${core}/lib/libfp6_wifi_qr.a"
      # this QtMultimedia, not the stock one a UITK dependency propagates
      "-DQt5Multimedia_DIR=${qtmultimedia.dev}/lib/cmake/Qt5Multimedia"
    ];
    qtWrapperArgs = lib.optionals (assignedMac != "") [ "--set-default FP6_WIFIQR_ASSIGNED_MAC ${assignedMac}" ];
    dontWrapQtApps = true;
    postInstall = ''
      install -Dm644 ../icons/fp6-wifi-qr-scan.svg $out/share/icons/hicolor/scalable/apps/fp6-wifi-qr-scan.svg
      mkdir -p $out/share/icons/hicolor/256x256/apps
      rsvg-convert -w 256 -h 256 ../icons/fp6-wifi-qr-scan.svg -o $out/share/icons/hicolor/256x256/apps/fp6-wifi-qr-scan.png
      install -Dm644 -t $out/share/fp6-wifi-qr/qml/icons ../icons/fp6-qrcode-symbolic.svg
      mkdir -p $out/share/applications
      # NoDisplay: opened from Settings > Wi-Fi and the network menu. Its
      # Name is what the camera in-use indicator shows.
      cat > $out/share/applications/fp6-wifi-qr-scan.desktop <<EOF
      [Desktop Entry]
      Type=Application
      Name=Wi-Fi QR scanner
      Comment=Scan a Wi-Fi QR code to join the network
      Exec=$out/bin/fp6-wifi-qr-scan
      Icon=$out/share/icons/hicolor/256x256/apps/fp6-wifi-qr-scan.png
      Terminal=false
      NoDisplay=true
      Categories=Network;
      X-Lomiri-Touch=true
      X-Lomiri-Single-Instance=true
      X-Lomiri-Supported-Orientations=portrait
      EOF
      sed -i 's/^      //' $out/share/applications/fp6-wifi-qr-scan.desktop
    '';
    # Wrapped by hand: exactly one QtMultimedia in the wrapper (this one).
    postFixup = ''
      saved=("''${qtWrapperArgs[@]}")
      qtWrapperArgs=()
      stockQtmm=${libsForQt5.qtmultimedia.bin}
      myQtmm=${qtmultimedia.bin}
      for a in "''${saved[@]}"; do
        qtWrapperArgs+=("''${a//"$stockQtmm"/"$myQtmm"}")
      done
      wrapQtApp $out/bin/fp6-wifi-qr-scan ${lib.escapeShellArgs cameraWrapArgs}
      test -x $out/bin/.fp6-wifi-qr-scan-wrapped
      grep -q ${qtmultimedia.bin} $out/bin/fp6-wifi-qr-scan
    '' + lib.optionalString hasCamera ''
      grep -q fp6camerabinsrc $out/bin/fp6-wifi-qr-scan
      ! grep -q ${builtins.substring 11 32 (toString libsForQt5.qtmultimedia.bin)} $out/bin/fp6-wifi-qr-scan
      # linked against the camera's QtMultimedia only
      ! readelf -d $out/bin/.fp6-wifi-qr-scan-wrapped | grep -q ${builtins.substring 11 32 (toString libsForQt5.qtmultimedia.out)}
    '';
    passthru = { inherit core hasCamera; };
    meta = {
      description = "Scan a Wi-Fi QR code to join the network (Lomiri, Fairphone 6)";
      license = lib.licenses.gpl3Only;
      platforms = lib.platforms.linux;
      mainProgram = "fp6-wifi-qr-scan";
    };
  };
in { inherit core lssPlugin scanner; }
