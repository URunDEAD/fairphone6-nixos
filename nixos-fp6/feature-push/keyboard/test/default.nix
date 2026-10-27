# Host-side checks for the keyboard work (build/eval only, no phone):
#   nix-build nixos-fp6/feature-push/keyboard/test -A maliit-keyboard  patched build; runs the
#                                                                       C++ predictor test + parity
#   nix-build nixos-fp6/feature-push/keyboard/test -A models           Rust unit tests + models
#   nix-build nixos-fp6/feature-push/keyboard/test -A layoutCheck      qmllint + check_layout.py
#   $(nix-build nixos-fp6/feature-push/keyboard/test -A render)/bin/fp6-keyboard-render OUTDIR
{ nixpkgs ? (import ../../../upstream/mobile-nixos/npins).nixpkgs }:
let
  pkgs = import nixpkgs { overlays = [ (import ../overlay.nix { }) ]; };
  inherit (pkgs) lib;
  patchedSrc = pkgs.applyPatches {
    name = "maliit-keyboard-fp6-src";
    inherit (pkgs.maliit-keyboard) src;
    patches = [
      ../maliit-keyboard-ngram-prediction.patch
      ../maliit-keyboard-ro-layout-space-swipe-mic-slot.patch
    ];
  };
in rec {
  inherit (pkgs) maliit-keyboard;
  inherit (pkgs.maliit-keyboard.passthru.fp6Keyboard) models dicts;
  tool = models.passthru.tool;

  layoutCheck = pkgs.runCommand "fp6-keyboard-layout-check" {
    nativeBuildInputs = [ pkgs.python3 pkgs.libsForQt5.qtdeclarative.dev ];
  } ''
    cd ${patchedSrc}
    qmllint plugins/ro/qml/*.qml plugins/en/qml/*.qml qml/keys/*.qml
    python3 -I ${./check_layout.py} --self-test plugins/ro/qml/*.qml plugins/en/qml/*.qml | tee $out
  '';

  render = pkgs.writeShellApplication {
    name = "fp6-keyboard-render";
    runtimeInputs = with pkgs; [ xorg.xorgserver xorg.xdpyinfo xorg.xwininfo xorg.xwd xcompmgr xdotool imagemagick dbus coreutils gnugrep gawk ];
    runtimeEnv = {
      MALIIT_KEYBOARD = "${maliit-keyboard}";
      MALIIT_FRAMEWORK = "${pkgs.maliit-framework}/${pkgs.libsForQt5.qtbase.qtPluginPrefix}";
      QT_BIN_PLUGINS = "${pkgs.libsForQt5.qtbase.bin}/${pkgs.libsForQt5.qtbase.qtPluginPrefix}";
      QT_QML = "${pkgs.libsForQt5.qtdeclarative.bin}/${pkgs.libsForQt5.qtbase.qtQmlPrefix}";
      QMLSCENE = "${pkgs.libsForQt5.qtdeclarative.dev}/bin/qmlscene";
      CLIENT_QML = "${./render-client.qml}";
      SURU_ICONS = "${pkgs.lomiri.suru-icon-theme}";
      FONTCONFIG_FILE = "${pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts pkgs.ubuntu-classic ]; }}";
    };
    # Coordinates are passed as split "x y" words on purpose.
    excludeShellChecks = [ "SC2046" "SC2086" ];
    text = builtins.readFile ./render.sh;
  };
}
