# xdg-desktop-portal-lomiri (Rust, zbus) + fp6-file-picker (Qt 5.15 QML host
# with the Lomiri UI Toolkit picker). One output:
#   libexec/xdg-desktop-portal-lomiri          FileChooser backend
#   bin/fp6-file-picker                         the picker app (Lomiri window)
#   share/xdg-desktop-portal/portals/lomiri.portal
#   share/dbus-1/services/org.freedesktop.impl.portal.desktop.lomiri.service
#   lib/systemd/user/xdg-desktop-portal-lomiri.service
#   share/applications/fp6-file-picker.desktop  (NoDisplay; DESKTOP_FILE_HINT target)
{ lib
, stdenv
, rustPlatform
, cmake
, libsForQt5
, lomiri
}:
let
  portal = rustPlatform.buildRustPackage {
    pname = "xdg-desktop-portal-lomiri-backend";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./portal;
      filter = path: _type: baseNameOf path != "target";
    };
    cargoLock.lockFile = ./portal/Cargo.lock;
    # cargo test: MIME -> glob mapping, glob -> regex, user-dirs and xbel
    # parsing, a{sv} option parsing, results (uris/current_filter/choices),
    # unique names for SaveFiles.
    doCheck = true;
    meta.mainProgram = "xdg-desktop-portal-lomiri";
  };
in
stdenv.mkDerivation {
  pname = "xdg-desktop-portal-lomiri";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ./.;
    filter = path: type:
      let rel = lib.removePrefix (toString ./. + "/") (toString path); in
      lib.any (p: rel == p || lib.hasPrefix (p + "/") rel) [ "picker" "qml" "data" ];
  };

  cmakeDir = "../picker";

  nativeBuildInputs = [ cmake libsForQt5.wrapQtAppsHook ];
  buildInputs = with libsForQt5; [
    qtbase
    qtdeclarative          # QtQuick, Qt.labs.folderlistmodel
    qtwayland
    qtgraphicaleffects
    qtsvg
    lomiri.lomiri-ui-toolkit
    lomiri.lomiri-content-hub   # Lomiri.Content (Other apps)
    lomiri.suru-icon-theme
  ];

  postInstall = ''
    install -Dm755 ${portal}/bin/xdg-desktop-portal-lomiri $out/libexec/xdg-desktop-portal-lomiri
    install -Dm644 ../data/lomiri.portal $out/share/xdg-desktop-portal/portals/lomiri.portal
    for f in \
      share/applications/fp6-file-picker.desktop:fp6-file-picker.desktop \
      share/dbus-1/services/org.freedesktop.impl.portal.desktop.lomiri.service:org.freedesktop.impl.portal.desktop.lomiri.service \
      lib/systemd/user/xdg-desktop-portal-lomiri.service:xdg-desktop-portal-lomiri.service; do
      install -Dm644 "../data/''${f#*:}" "$out/''${f%%:*}"
      substituteInPlace "$out/''${f%%:*}" --replace-fail @out@ "$out"
    done
  '';

  # wrapQtAppsHook wraps bin/ and libexec/: the backend then finds the
  # picker and this package's desktop file (and passes the Qt/QML paths on).
  preFixup = ''
    qtWrapperArgs+=(
      --set-default FP6_FILE_PICKER $out/bin/fp6-file-picker
      --set-default FP6_FILE_PICKER_DESKTOP $out/share/applications/fp6-file-picker.desktop
    )
  '';

  passthru = { inherit portal; };

  meta = {
    description = "xdg-desktop-portal FileChooser backend with a Lomiri UI Toolkit picker (FP6)";
    license = lib.licenses.gpl3Only;
    platforms = lib.platforms.linux;
    mainProgram = "fp6-file-picker";
  };
}
