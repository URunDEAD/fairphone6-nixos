# FP6 USB Drive: Rust crate (helper daemon, session agent, CLI and the
# app backend as a static library) + a one-class Qt 5.15 shim (app/) + Lomiri
# UI Toolkit QML (qml/).
{ lib
, stdenv
, rustPlatform
, cmake
, pkg-config
, glib
, libsForQt5
, lomiri
, librsvg
, dosfstools
, exfatprogs
, xz
, gzip
, zstd
, bzip2
, curl
}:
let
  core = rustPlatform.buildRustPackage {
    pname = "fp6-usb-drive-core";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./.;
      filter = path: type:
        let rel = lib.removePrefix (toString ./. + "/") (toString path); in
        !(lib.hasInfix "/target" rel)
        && lib.any (p: rel == p || lib.hasPrefix (p + "/") rel) [ "rust" "downloads.json" ];
    };
    sourceRoot = "source/rust";
    cargoLock.lockFile = ./rust/Cargo.lock;
    nativeBuildInputs = [ pkg-config glib ];
    buildInputs = [ glib ];
    # cargo test: SHA-256 vectors, ISO/El Torito/MBR/GPT/FAT detection, sums
    # files, catalog globbing, safe open (symlinks, hard links, owner), gadget
    # host/restore/exclusive/rollback on a fake configfs, create/decompress.
    doCheck = true;
    nativeCheckInputs = [ gzip dosfstools ];
    postInstall = ''
      lib=$(find target -path '*release/libfp6_usbdrive.a' | head -n1)
      test -n "$lib"
      install -Dm644 "$lib" $out/lib/libfp6_usbdrive.a
      install -Dm644 ../downloads.json $out/share/fp6-usb-drive/downloads.json
    '';
    meta.mainProgram = "fp6-usb-drived";
  };
  toolPath = lib.makeBinPath [ dosfstools exfatprogs xz gzip zstd bzip2 curl ];
in
stdenv.mkDerivation {
  pname = "fp6-usb-drive";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type:
      let rel = lib.removePrefix (toString ./. + "/") (toString path); in
      lib.any (p: rel == p || lib.hasPrefix (p + "/") rel) [ "app" "qml" "icons" ];
  };

  cmakeDir = "../app";
  nativeBuildInputs = [ cmake libsForQt5.wrapQtAppsHook librsvg ];
  buildInputs = with libsForQt5; [
    qtbase
    qtdeclarative
    qtgraphicaleffects
    qtsvg
    lomiri.lomiri-ui-toolkit
  ];
  cmakeFlags = [ "-DFP6_USBDRIVE_LIB=${core}/lib/libfp6_usbdrive.a" ];

  qtWrapperArgs = [
    "--prefix PATH : ${toolPath}"
    "--set-default FP6_USB_DRIVE_CATALOG /etc/fp6-usb-drive/downloads.json"
  ];

  postInstall = ''
    install -Dm644 ../icons/fp6-usb-drive.svg $out/share/icons/hicolor/scalable/apps/fp6-usb-drive.svg
    mkdir -p $out/share/fp6-usb-drive
    rsvg-convert -w 256 -h 256 ../icons/fp6-usb-drive.svg -o $out/share/fp6-usb-drive/fp6-usb-drive.png
    install -Dm644 $out/share/fp6-usb-drive/fp6-usb-drive.png $out/share/icons/hicolor/256x256/apps/fp6-usb-drive.png
    for b in fp6-usb-drived fp6-usb-drive-agent fp6-usb-drive-cli; do
      ln -s ${core}/bin/$b $out/bin/$b
    done
    install -Dm644 ${core}/share/fp6-usb-drive/downloads.json $out/share/fp6-usb-drive/downloads.json
  '';

  passthru = { inherit core; };

  meta = {
    description = "Share ISO/IMG files on the Fairphone 6 as a USB CD-ROM or disk (DriveDroid-like) for Lomiri";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "fp6-usb-drive";
  };
}
