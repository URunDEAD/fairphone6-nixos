# FP6 Bluetooth settings (see bluetooth-settings-fp6.nix).
#  lssBluetooth: the stock System Settings Bluetooth panel (plugins/bluetooth
#                of lomiri-system-settings) rebuilt out of tree with
#                lss-bluetooth-fp6.patch: the module's files replace the
#                stock ones in the lomiri-system-settings wrapper, so neither
#                lomiri-system-settings-unwrapped nor Lomiri is rebuilt.
#  indicator:    ayatana-indicator-bluetooth with indicator-bluetooth-fp6.patch
#                (paired devices with connect switches in the phone menu).
#  icons:        fp6-audio-earbuds-symbolic (hicolor; Suru has no earbuds).
{ lib, stdenv, cmake, libsForQt5, lomiri, ayatana-indicator-bluetooth, runCommand }:
let
  lssUnwrapped = lomiri.lomiri-system-settings-unwrapped;

  lssBluetooth = stdenv.mkDerivation {
    pname = "lomiri-system-settings-bluetooth-fp6";
    inherit (lssUnwrapped) version src;

    patches = [ ./lss-bluetooth-fp6.patch ];

    # Only the Bluetooth plugin, with the install layout of the full build
    # (PLUGIN_*_DIR as in lomiri-system-settings' CMakeLists.txt).
    postPatch = ''
      cat > CMakeLists.txt <<'CMAKE'
      cmake_minimum_required(VERSION 3.16)
      project(lss-bluetooth-fp6 CXX)
      set(CMAKE_CXX_STANDARD 17)
      set(CMAKE_AUTOMOC ON)
      set(CMAKE_CXX_FLAGS "''${CMAKE_CXX_FLAGS} -fno-permissive -Wall -Wextra")
      find_package(Qt5 REQUIRED COMPONENTS Core DBus Qml Quick)
      set(PLUGIN_PRIVATE_MODULE_DIR "''${CMAKE_INSTALL_PREFIX}/lib/lomiri-system-settings/private")
      set(PLUGIN_QML_DIR "''${CMAKE_INSTALL_PREFIX}/share/lomiri-system-settings/qml-plugins")
      set(PLUGIN_MANIFEST_DIR "''${CMAKE_INSTALL_PREFIX}/share/lomiri-system-settings")
      add_subdirectory(plugins/bluetooth)
      CMAKE
    '';

    strictDeps = true;
    nativeBuildInputs = [ cmake ];
    buildInputs = [ libsForQt5.qtbase libsForQt5.qtdeclarative ];
    dontWrapQtApps = true;

    postInstall = ''
      m=$out/lib/lomiri-system-settings/private/Lomiri/SystemSettings/Bluetooth
      q=$out/share/lomiri-system-settings/qml-plugins/bluetooth
      test -e $m/libLomiriBluetoothPanel.so
      test -e $m/qmldir
      for f in PageComponent.qml DevicePage.qml Fp6RenameDialog.qml Fp6DeviceTypePage.qml Fp6Kinds.js; do
        test -e $q/$f
      done
      grep -q fp6PhoneName $q/PageComponent.qml
    '';

    meta = lssUnwrapped.meta // {
      description = "lomiri-system-settings Bluetooth panel with the FP6 additions";
    };
  };

  indicator = ayatana-indicator-bluetooth.overrideAttrs (old: {
    pname = "${old.pname}-fp6";
    patches = (old.patches or [ ]) ++ [ ./indicator-bluetooth-fp6.patch ];
  });

  icons = runCommand "fp6-bluetooth-icons" { } ''
    install -Dm644 ${./icons/fp6-audio-earbuds-symbolic.svg} \
      $out/share/icons/hicolor/scalable/devices/fp6-audio-earbuds-symbolic.svg
  '';
in
{
  inherit lssBluetooth indicator icons;
}
