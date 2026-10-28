# Host-side checks for the keyboard work (build/eval only, no phone):
#   nix-build nixos-fp6/feature-push/keyboard/test -A maliit-keyboard  patched build; runs the
#                                                                       C++ predictor test + parity
#   nix-build nixos-fp6/feature-push/keyboard/test -A models           Rust unit tests + models
#   nix-build nixos-fp6/feature-push/keyboard/test -A layoutCheck      qmllint + check_layout.py
#   (the maliit-keyboard build also runs the dictation bridge test against a fake service)
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
      ../maliit-keyboard-dictation-key.patch
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
    qmllint plugins/ro/qml/*.qml plugins/en/qml/*.qml qml/keys/*.qml ${../settings-page}/PageComponent.qml
    python3 -I ${./check_layout.py} --self-test plugins/ro/qml/*.qml plugins/en/qml/*.qml | tee $out
  '';

  # Stand-in for com.fairphone6.Dictation1 (synthetic text, no microphone).
  fakeDictation = pkgs.stdenv.mkDerivation {
    pname = "fp6-fake-dictation";
    version = "1";
    src = patchedSrc;
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.libsForQt5.qtbase ];
    dontWrapQtApps = true;
    buildPhase = ''
      $CXX -std=c++17 -fPIC -Itests/fp6 tests/fp6/fake_dictation_main.cpp \
        $(pkg-config --cflags --libs Qt5DBus Qt5Core) -o fp6-fake-dictation
    '';
    installPhase = "install -Dm755 fp6-fake-dictation $out/bin/fp6-fake-dictation";
    meta.mainProgram = "fp6-fake-dictation";
  };

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
      FAKE_DICTATION = lib.getExe fakeDictation;
      SURU_ICONS = "${pkgs.lomiri.suru-icon-theme}";
      FONTCONFIG_FILE = "${pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts pkgs.ubuntu-classic ]; }}";
    };
    # Coordinates are passed as split "x y" words on purpose.
    excludeShellChecks = [ "SC2046" "SC2086" ];
    text = builtins.readFile ./render.sh;
  };
}
