# Laptop check that the Lomiri overlays of the ENABLED trial target
# (refresh-rate/trial-kernel6-refresh-rate-fp6.nix) apply: replays the
# aarch64 lomiri derivation's patches + postPatch on x86_64 (text edits
# only, nothing is compiled) and checks the result with qmllint. Also
# applies lss-brightness-smooth-display.patch to the stock brightness page
# of the target's lomiri-system-settings source (done in the wrapper there).
#   nix-build nixos-fp6/feature-push/refresh-rate/test/lomiri-patch-check.nix
let
  trial0 = import ../trial-kernel6-refresh-rate-fp6.nix { };
  # Modules whose Lomiri postPatch copies aarch64 build outputs (not
  # buildable here) are switched off for the replay; they touch none of
  # the files edited by refresh-rate (lock sounds, 2026-10-28).
  trial = trial0.eval.extendModules {
    modules = [ ({ lib, ... }: { hardware.fairphone6.lockSounds.enable = lib.mkForce false; }) ];
  };
  lomiri = trial.pkgs.lomiri.lomiri;
  lssSrc = trial.pkgs.lomiri.lomiri-system-settings-unwrapped.src;
  pkgs = import trial0.pkgs.path { system = "x86_64-linux"; };
in
pkgs.stdenv.mkDerivation {
  name = "fp6-refresh-rate-lomiri-patch-check";
  inherit (lomiri) src;
  patches = lomiri.patches or [ ];
  postPatch = lomiri.postPatch or "";
  nativeBuildInputs = [ pkgs.libsForQt5.qtdeclarative.dev ];
  dontConfigure = true;
  dontBuild = true;
  dontWrapQtApps = true;
  installPhase = ''
    grep -q 'Fp6RefreshRate {' qml/ShellScreen.qml
    grep -q 'fp6RefreshRateTile\]' qml/Panel/QuickSettings/QsBackend.qml
    test -e qml/Fp6RefreshRate.qml && test -e qml/Fp6RefreshRate.js
    qmllint qml/ShellScreen.qml qml/Fp6RefreshRate.qml qml/Panel/QuickSettings/QsBackend.qml
    cp ${lssSrc}/plugins/brightness/PageComponent.qml brightness-PageComponent.qml
    chmod u+w brightness-PageComponent.qml
    patch brightness-PageComponent.qml < ${../lss-brightness-smooth-display.patch}
    grep -q Fp6SmoothDisplay.qml brightness-PageComponent.qml
    qmllint brightness-PageComponent.qml
    mkdir -p $out
    cp brightness-PageComponent.qml $out/
    cp qml/ShellScreen.qml qml/Panel/QuickSettings/QsBackend.qml $out/
  '';
}
