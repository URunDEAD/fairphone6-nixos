# Laptop (x86) builds for the text-size layout checks: the x86 UITK with
# uitk-font-scale.patch (tests skipped) as fp6-uitk-font-scale-lib, a
# qmlscene wrapper with the UITK/Qt QML paths and Ubuntu fonts, and System
# Settings with the Display Comfort plugin and lss-text-size.patch.
# Build/eval only; nothing here runs on or talks to the phone.
let
  root = ../../../../nixos-fp6;
  pkgs = import (import (root + "/upstream/mobile-nixos/npins")).nixpkgs { system = "x86_64-linux"; };
  inherit (pkgs) lib;
  dc = root + "/feature-push/display-comfort";
  qt = pkgs.libsForQt5;
  uitk = pkgs.lomiri.lomiri-ui-toolkit.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ (dc + "/uitk-font-scale.patch") ];
    doCheck = false;
  });
  stockUitk = pkgs.lomiri.lomiri-ui-toolkit;
in rec {
  inherit pkgs uitk;
  fontScaleLib = pkgs.runCommand "fp6-uitk-font-scale-lib" { } ''
    mkdir -p $out/lib
    ln -s ${uitk}/lib/libLomiriToolkit.so.5 $out/lib/libLomiriToolkit.so.5
  '';
  qmlscene = let
    qml = lib.concatMapStringsSep ":" (p: "${lib.getBin p}/lib/qt-5.15.19/qml")
      [ stockUitk qt.qtdeclarative qt.qtgraphicaleffects qt.qtfeedback qt.qtpim qt.qtsystems qt.qtsvg ];
    plugins = lib.concatMapStringsSep ":" (p: "${lib.getBin p}/lib/qt-5.15.19/plugins")
      [ qt.qtbase qt.qtdeclarative qt.qtsvg stockUitk qt.qtpim ];
    fonts = pkgs.makeFontsConf { fontDirectories = [ pkgs.ubuntu-classic pkgs.dejavu_fonts ]; };
  in pkgs.writeShellScript "fp6-qmlscene" ''
    export QML2_IMPORT_PATH=${qml} QT_PLUGIN_PATH=${plugins} FONTCONFIG_FILE=${fonts}
    exec ${lib.getDev qt.qtdeclarative}/bin/qmlscene "$@"
  '';
  lss = (pkgs.lomiri.overrideScope (_: lprev: {
    lomiri-system-settings-unwrapped = lprev.lomiri-system-settings-unwrapped.overrideAttrs (o: {
      patches = (o.patches or [ ]) ++ [ (dc + "/lss-text-size.patch") ];
    });
  })).lomiri-system-settings.override (old: {
    plugins = (old.plugins or [ ]) ++ [ (pkgs.callPackage (dc + "/lomiri-system-settings-display-comfort.nix") { }) ];
  });
}
