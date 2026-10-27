# OTA updates (feature push 2026-10-24).
#  core:      Rust crate (rust/): fp6-updated (phone: D-Bus service, health
#             check, read-only slot/pair reports) and fp6-release (release
#             side: keys, publish to a binary cache, manifest signing, serve).
#  lssPlugin: System Settings > System update (UITK QML) and the Fp6.Update
#             QML module (one Qt D-Bus bridge class + UpdateModel.qml).
{ lib, stdenv, rustPlatform, cmake, libsForQt5 }:
let
  core = rustPlatform.buildRustPackage {
    pname = "fp6-ota";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./rust;
      filter = path: _type: baseNameOf path != "target";
    };
    cargoLock.lockFile = ./rust/Cargo.lock;
    # cargo test (synthetic data only): minisign-format signing/verification
    # and tampering, manifest/closure validation, policy, health decisions,
    # qbootctl dump parsing, GPT A/B bits on synthetic disks, boot image
    # backup/write/readback on files, slot pair verdicts, state file.
    doCheck = true;
    meta = {
      description = "Signed OTA updates with A/B rollback for NixOS on the Fairphone 6";
      license = lib.licenses.mit;
      mainProgram = "fp6-updated";
      platforms = lib.platforms.linux;
    };
  };

  lssPlugin = stdenv.mkDerivation {
    pname = "lomiri-system-settings-update";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./.;
      filter = path: type:
        let rel = lib.removePrefix (toString ./. + "/") (toString path); in
        lib.any (p: rel == p || lib.hasPrefix (p + "/") rel) [ "qml-plugin" "settings-page" ];
    };
    sourceRoot = "source/qml-plugin";
    nativeBuildInputs = [ cmake ];
    buildInputs = [ libsForQt5.qtbase libsForQt5.qtdeclarative ];
    dontWrapQtApps = true;
    postInstall = ''
      install -Dm644 $NIX_BUILD_TOP/source/settings-page/update/update.settings $out/share/lomiri-system-settings/update.settings
      install -Dm644 -t $out/share/lomiri-system-settings/qml-plugins/update $NIX_BUILD_TOP/source/settings-page/update/*.qml
      mkdir -p $out/share/locale
      test -e $out/lib/lomiri-system-settings/private/Fp6/Update/libfp6updateplugin.so
    '';
    meta = {
      description = "System update page for Lomiri System Settings";
      license = lib.licenses.gpl3Only;
      platforms = lib.platforms.linux;
    };
  };
in { inherit core lssPlugin; }
